# Executor integration and window management

Date: 2026-11-25. Spec source: Jimmy's message in the project thread (sections 1 to 6).

## Hard limits

Dockie never injects into Roblox, never touches the executor's files or binary, never touches
anti-cheat. It only starts the executor process, writes its own `dockie_antiafk.lua`, runs a
localhost WebSocket server, and changes window styles with plain Win32 calls
(`SetWindowLongPtrW`, `ShowWindow`, `ITaskbarList`).

## Where things live (existing conventions)

- Global settings: `store::Settings` in `%LOCALAPPDATA%\Dockie\settings.json`, saved by
  `set_settings`, edited on the Settings tab. New fields use `#[serde(default)]` so old files load.
- Per-account settings: `store::Account` in `accounts.json`, edited in the Edit sheet.
  `upsert` keeps user fields on re-sign-in; the new flag joins that list. Export/import leave it out
  (it is a local window preference, not account data).
- Runtime state: `store::State` in `state.json` (launched PIDs). Window styles Dockie changed are
  recorded here too, so a crash can be undone on the next start.
- Tracking: `ticker.rs` already runs once a second over every live Dockie-launched Roblox PID and
  its `procs::main_window`. Relaunches and rejoins with a new PID or window are handled there
  because the ticker re-finds the window every tick.

## New settings (global)

| field | default | meaning |
|---|---|---|
| `executor_path` | `""` | full path to the executor exe; its file name is the process name |
| `executor_autostart` | `false` | start the executor before launching accounts |
| `autoexec_dir` | `""` | executor's autoexec folder |
| `ws_port` | `47613` | localhost WebSocket port, templated into the script |
| `manage_executor_windows` | `true` | hide executor windows from Alt+Tab and taskbar, minimize to tray |

Per-account: `hide_alt_tab: bool` (default false).

## 1. Executor launch (`executor.rs`)

`ensure_running()` runs when Dockie starts and in the launcher loop before each job, when
`executor_autostart` is on and the path is set. If a process with the exe's file name is already
running, nothing happens. Otherwise it starts it with `CreateProcessW`, `STARTF_USESHOWWINDOW |
SW_SHOWMINNOACTIVE`, and the environment variable `__COMPAT_LAYER=RunAsInvoker` (grill decision: Potassium's
manifest asks for admin; RunAsInvoker runs it as a normal app, so no UAC prompt and Dockie may change
its windows; nothing in Potassium's files changes). Then it polls the process list every 250 ms for up
to 10 s. If an executor that is already running turns out to be elevated (style writes fail with
access denied), Dockie leaves it alone, logs it once and shows a notice: close it and let Dockie start it. A missing or wrong path, or a timeout, is logged and shown as a
toast; launching Roblox continues anyway (the executor is optional to the launch).

## 2. Autoexec script

Written on startup and whenever `autoexec_dir` or `ws_port` change. Dockie renders the template,
compares with the file on disk and only writes when different. It never lists, reads or deletes
any other file in the folder. A missing folder is logged, not created.

The Luau waits for `game:IsLoaded()` and `Players.LocalPlayer`, disables every `Idled` connection via
`getconnections` inside `pcall`, then (only if `WebSocket.connect` exists) loops: connect, send hello
`{type="hello", userId, username, placeId, jobId}`, heartbeat `{type="heartbeat", userId}` every 10 s,
reconnect 15 s after any failure. Everything in `pcall`.

## 3. WebSocket server (`attach.rs`)

`tungstenite` (sync) on a plain thread: `TcpListener` bound to `127.0.0.1:<ws_port>`, one thread per
client. Default port 47613, no path checked (Dockie Remote's phone server will use 47821 and `/v1`;
the two stay separate servers on separate ports). Handshake rejects any request with an `Origin` header (browsers always send one, executors
don't), so a web page can't fake statuses. Max 32 clients, 4 KB max message. Malformed JSON is ignored.
Each connection gets an id; `userId -> (conn_id, last_seen, closed)`. A close only marks
Disconnected if it is still the newest connection for that user (rejoins open a new one first).
Status: Attached on hello/heartbeat, Stale when `last_seen` is older than 30 s (computed in the
ticker), Disconnected on close. Unknown userIds are logged and ignored. Port in use: logged, toast,
the rest of Dockie works. Changing the port in Settings restarts the listener.

UI: the account tile shows a small pill next to the run state (Attached / Stale / Disconnected).
Nothing is shown for an account that never said hello.

## 4. Executor windows (`winstyle.rs` + ticker)

Each tick (1 s), when `manage_executor_windows` is on and the executor is running: `EnumWindows` +
`GetWindowThreadProcessId` for its PID, only visible, unowned top-level windows (hidden helper windows
are never touched, so restoring never shows something that was meant to be hidden). For each new
window: save the original `GWL_EXSTYLE`, set `WS_EX_TOOLWINDOW`, clear `WS_EX_APPWINDOW`, then
`SW_HIDE` + `SW_SHOWNA` so the shell drops the button. If a later tick sees the style reset, it applies
it again.

Minimize to tray: a "hooks" thread with a message loop owns `SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART,
…, executor_pid, 0, WINEVENT_OUTOFCONTEXT)`. On minimize it `SW_HIDE`s the window. Grill decision: the
tray icon (Dockie's icon, tooltip with the exe name) shows the whole time the executor is running and
managed, since with no taskbar button and no Alt+Tab it is the only way back to the executor.
Left-click toggles: hidden or not in front, it is shown (`SW_RESTORE` + `SetForegroundWindow`); in
front, it is hidden. Menu: Show / Hide / Exit executor. Exit closes the executor's
windows with `WM_CLOSE` (graceful, not a kill). The hook is re-installed when the executor PID changes.

If the executor runs as administrator, Windows blocks style changes from non-admin Dockie (UIPI):
logged once, feature skipped.

## 5. Per-account Hide from Alt+Tab

One function `winstyle::set_alt_tab_hidden(hwnd, bool)`. Each tick, for each live launched account
with `hide_alt_tab`, the main window gets the treatment; with the flag off and a saved original, the
exact original `GWL_EXSTYLE` goes back. Toggling in the Edit sheet saves the account and the next
tick applies it (within 1 s).

Approach (a): set `WS_EX_TOOLWINDOW`, then `ITaskbarList::HrInit` + `AddTab(hwnd)`. The Dockie main
window's `TaskbarCreated` message (shell restart) triggers `AddTab` again for every hidden window.
Approach (b): `WS_EX_TOOLWINDOW | WS_EX_APPWINDOW`. Per Raymond Chen's Alt+Tab rules, APPWINDOW
forces a window into Alt+Tab, so (b) is expected to fail; tested anyway on a test window.
Result: see "Test results" below.

## 6. Cleanup

Every changed window is recorded in `State.styled: [{hwnd, pid, original_exstyle, hidden}]` and saved.
On `RunEvent::Exit`, and from a panic hook, Dockie restores every recorded window that still belongs
to the same PID (`IsWindow` + PID check, so a recycled HWND is never touched), shows windows it hid,
removes the tray icon and unhooks. A hard crash can't run code, so the next start does the same
restore from `state.json` before anything else.

## Logging

Dockie had no log. New `log.rs`: one line per event appended to `%LOCALAPPDATA%\Dockie\dockie.log`,
truncated to empty when it passes 1 MB. Logged: executor start/skip/fail, script written/unchanged,
WS listen/fail, hello/close per user, style applied/restored/blocked.

## Facts found on Jimmy's PC

- Executor: Potassium, `C:\Users\ytkjp\Desktop\games\OpXOyuApWKTlFzrV\Potassium.exe`, process name
  `Potassium.exe`. Autoexec folder: `%LOCALAPPDATA%\Potassium\autoexec` (already holds Jimmy's own
  `antiafk.luau`, left alone).
- Potassium's manifest is `requireAdministrator`. A no-op `SetWindowLongPtrW(GWL_EXSTYLE)` from a normal
  process on its windows fails with error 5 (UIPI). So section 4 needs Potassium to run un-elevated.

## Test results

Test window (a WinForms window made by a scratch script), Alt+Tab captured with a screenshot:
- no change: in Alt+Tab.
- (a) `WS_EX_TOOLWINDOW`, `WS_EX_APPWINDOW` cleared: gone from Alt+Tab.
- (b) `WS_EX_TOOLWINDOW | WS_EX_APPWINDOW`: still in Alt+Tab. (b) fails, as Raymond Chen's rules predict.

Taskbar side of (a) with `AddTab`: (filled in after the Rust build)

## Still open

Nothing blocking. Not verifiable without running things on Jimmy's PC (he tests): whether Potassium
works fully under RunAsInvoker, and the taskbar side of approach (a).
