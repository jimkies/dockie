# Dockie — design

Date: 2026-11-18 · Status: awaiting approval

## Intent

Dockie is a Windows app for keeping several Roblox accounts saved and opening many of them at once, each in its own Roblox window. It is for personal use and close friends, so a few accounts per person rather than hundreds.

It is a working copy of ExoPanda Multi 1.1.7 (a Tauri app; its feature list was read out of its program file). The macro features, the license key and the bootstrapper are left out. A few things ExoPanda lacks are added: every join type, "Join selected into…", renamed windows, import/export, and an option to free RAM from minimized windows.

## Goals

1. **Add accounts in two ways**
   - **Sign in:** opens Roblox's real login page in its own private window. Once you sign in, Dockie reads the session cookie, saves the account and closes the window.
   - **Paste cookie:** you paste a `.ROBLOSECURITY` value. Dockie checks it with Roblox before saving.
2. **Account list:** each row has a checkbox, avatar, display name and @username, a status badge, a live preview, a Join box, an FPS cap override, Launch/Stop buttons, an editable note, and Remove. The list stays in the order accounts were added.
3. **Launching:** Launch (one account), Launch selected, Stop, Stop all. Launches run one at a time, and Dockie waits for each window before starting the next.
4. **Multiple windows are always on** while Dockie is open. There is no switch.
5. **Smart Join box per account:** it accepts a place ID, game link, private server link (old or new style), a link to a specific server, a profile link or a username. Empty means the Roblox home screen. Below the box, Dockie shows what it understood ("Adopt Me!", "Private server in Adopt Me!", "Follow @name"…).
6. **"Join selected into…" box at the top:** one target for all ticked accounts. It overrides their own Join boxes for that launch only.
7. **Live previews:** a thumbnail about every second for each Roblox window Dockie launched. Click it to bring that window to the front. A minimized window shows "minimized".
8. **Renamed windows:** each Roblox window Dockie launches is titled `Roblox · <username>`.
9. **FPS cap:** a global default in Settings, with an optional override per account. It is **Dockie-only**: your normal Roblox keeps its own value (see FPS cap below).
10. **Check on startup:** every account is checked with Roblox when Dockie opens. Names and avatars are refreshed, and expired sessions get a "Needs sign-in" badge with a **Sign in again** button.
11. **Import/export:** accounts move between PCs, for example to a friend, as one JSON file.
12. **Tools tab:** lists every running Roblox window, including ones Dockie didn't launch, labelled with the account where known. Each has Focus and Close, plus a **Close all Roblox** button.
13. **Roblox already running when Dockie opens:** a warning bar with a **Close all Roblox** button.
14. **Quitting:** quitting asks first if windows Dockie launched are still open. They stay open, but no further extra windows can open until Dockie is reopened.
15. **Distribution:** one portable `dockie.exe`, with no installer and no auto-update.
16. **Free RAM from minimized windows (setting, off by default):** Windows moves the idle memory of minimized Dockie windows out of RAM and into the page file on the SSD. Each window can drop from about 3 GB toward a few hundred MB. Opening the window again brings a short stutter while the memory loads back.

## Non-goals

These are left out on purpose:

- Macros, input recording, cycling macros between accounts, and global hotkeys.
- License keys, the ExoPanda website, and any account or server of our own.
- A bootstrapper (installing, updating or repairing Roblox). Roblox's official launcher already does this.
- Window tiling.
- Fast flags (Roblox now only accepts an approved list, and you decided they aren't worth it).
- A system tray icon, auto-update, an installer, drag-to-reorder, and an on/off switch for multiple windows.

## Anti-goals (failure even if it "works")

- A Roblox started outside Dockie ends up with a different FPS cap because of Dockie.
- A cookie is sent anywhere other than Roblox's own `*.roblox.com` addresses.
- Dockie sends any mouse or keyboard input to a game.

## Constraints and known facts

- **Platform:** Windows 10/11 only. WebView2 is already installed there.
- **Toolchain:** Rust 1.99.1, Node 26 and Tauri 2.12.6 (the latest on docs.rs). The Tauri webview can read every cookie, including HTTP-only ones (`Webview::cookies_for_url(url)`). The login window is built with `WebviewWindowBuilder::incognito(true)`.
  - ⚠️ On Windows, creating windows or reading cookies from a **synchronous** command or event handler deadlocks (wry #583). Use async commands or separate threads.
- **Roblox install:** the `roblox-player` link handler points straight at `%LOCALAPPDATA%\Roblox\Versions\version-<hash>\RobloxPlayerBeta.exe "%1"`. Dockie reads that path from the registry (`HKCU\Software\Classes\roblox-player\shell\open\command`, falling back to `HKCR`). It starts the exe itself with the launch link as the argument, so it knows the process ID (PID). If no handler is found, it says "Roblox isn't installed".
- **Getting a launch ticket** (the same request ExoPanda makes):
  - `POST https://auth.roblox.com/v1/authentication-ticket/` with the cookie `.ROBLOSECURITY=<cookie>` and `Referer`/`Origin: https://www.roblox.com`.
  - The first reply is a 403 that carries an `x-csrf-token`. Send the request again with that token. The ticket comes back in the `rbx-authentication-ticket` reply header.
- **Launch link:**
  - Home screen: `roblox-player:1+launchmode:app+gameinfo:<ticket>+launchtime:<ms>+browsertrackerid:<n>+robloxLocale:en_us+gameLocale:en_us`
  - Game: the same with `launchmode:play` and `+placelauncherurl:<url-encoded>`, where the URL is `https://assetgame.roblox.com/game/PlaceLauncher.ashx?request=…&browserTrackerId=<n>&isPlayTogetherGame=false` plus:

    | Target | `request=` | Extra parameters |
    |---|---|---|
    | game | `RequestGame` | `placeId` |
    | private server | `RequestPrivateGame` | `placeId`, `linkCode` |
    | specific server | `RequestGameJob` | `placeId`, `gameId=<jobId>` |
    | follow user | `RequestFollowUser` | `userId` |
- **Multiple windows:** at startup Dockie creates and holds the named mutex `ROBLOX_singletonMutex`. Newer Roblox builds also need the event `ROBLOX_singletonEvent`. It holds them for its whole lifetime. If Roblox already owns them, multi-instance can't be enabled, and the warning bar appears. After a successful **Close all Roblox**, Dockie tries again. **Verify against the current client during testing.**
- **Roblox web addresses used:**
  - `users.roblox.com/v1/users/authenticated`: checks a cookie and gets the user id, name and display name.
  - `thumbnails.roblox.com/v1/users/avatar-headshot?userIds=…&size=150x150&format=Png`: avatar.
  - `users.roblox.com/v1/usernames/users` (POST): username to user id.
  - `apis.roblox.com/universes/v1/places/<placeId>/universe` and then `games.roblox.com/v1/games?universeIds=…`: game name.
  - `apis.roblox.com/sharelinks/v1/resolve-link` (POST `{linkId, linkType:"Server"}`, with cookie and CSRF token): turns a new-style private server link into `placeId` + `linkCode`.
- **FPS cap file:** `%LOCALAPPDATA%\Roblox\GlobalBasicSettings_13.xml` holds `<int name="FramerateCap">N</int>` (currently 240). Every Roblox window shares this file. Roblox reads it at start and writes it back when a window closes.

## Approaches considered

| Approach | Trade-off | Picked? |
|---|---|---|
| **Rust + Tauri 2, plain HTML/JS** | Same structure as ExoPanda. One small exe. Native screen capture, login webview and cookie API. More code and slower builds. | **Yes.** It matches the original, and the tools are already installed. |
| Python + pywebview | About half the code, one pip package. Runs as a script, so an exe needs PyInstaller. Clunkier previews. | No |
| Python + tkinter | No dependencies, but the login window would still need a webview, and the UI looks dated. | No |

## How it works

### Data (all under `%LOCALAPPDATA%\Dockie\`)

Every file is written atomically: Dockie writes a temporary file, then renames it over the old one.

- **`accounts.json`:** `{version:1, accounts:[{user_id, username, display_name, avatar_url, added_at, note, join, fps_cap|null, cookie_dpapi, status}]}`.
  - `cookie_dpapi` is the cookie encrypted with Windows' per-user encryption (`CryptProtectData`), stored as base64.
  - If decryption fails (another PC or Windows user), the status becomes "Needs sign-in".
- **`settings.json`:** `{fps_cap_default|null, trim_minimized:false}`. A `null` FPS default means Dockie never touches the FPS cap.
- **`state.json`:**
  - `user_fps_cap`: your own FPS value, remembered before Dockie's first write.
  - `restore_pending`: true while the file holds a Dockie value.
  - `launched`: `[{pid, started_at, user_id}]`, so Dockie still recognizes its windows after a restart. Matching on the process start time guards against a reused PID.

### Adding accounts

- **Sign in:**
  1. The window opens at `https://www.roblox.com/login` with the title "Sign in to Roblox — Dockie".
  2. An async task reads the cookies for `https://www.roblox.com` once a second.
  3. When `.ROBLOSECURITY` appears, Dockie checks it, saves the account and closes the window.
  4. If you close the window yourself, the sign-in is cancelled.
- **Paste:** the cookie is trimmed but kept whole, including Roblox's `_|WARNING:…|_` prefix. Dockie checks it, then saves it.
- **An account that's already saved** (same user id) has its saved cookie replaced. This is also how **Sign in again** works. If you sign in as a different account than the one being refreshed, it is added as a new account and you're told.

### Join box parsing (pure function with unit tests)

Rules are checked in this order:

1. **Empty:** home screen.
2. **Contains `placeId=X` plus `gameInstanceId=Y` or `jobId=Y`, or is written `X:Y`:** that specific server.
3. **`…/games/X…?privateServerLinkCode=C`:** private server (X, C).
4. **`…/share?code=C&type=Server`:** a new-style private server link. It's turned into a server at launch time using the launching account's cookie.
5. **`…/games/X…` or digits only:** that game.
6. **`…/users/N/profile`:** follow user N.
7. **`@name`, or 3–20 characters of `[A-Za-z0-9_]`:** username, looked up and then followed. Use `@` to force a username that is all digits.
8. **Anything else:** an inline error, "Couldn't read this link".

A successful lookup shows what it found (game name, "Follow @name") under the box. Lookups are cached for the session.

### Launch queue

There is one queue for the whole app. Launch, Launch selected and Join selected into… all add to it. For each account:

1. Skip it if its window is already running (focus that window instead).
2. Get a launch ticket. If the cookie is rejected, mark it "Needs sign-in".
3. **FPS (only if an override or global default is set):**
   - Read the file. If `restore_pending` is false, save the current value as `user_fps_cap`.
   - Write the account's value and set `restore_pending`.
4. Start `RobloxPlayerBeta.exe <uri>` and record its PID in `launched`.
5. Wait up to 60 s for a visible top-level window belonging to that PID.
   - Roblox may hand off to a child process, so also match child processes. **Verify during testing.**
6. Put the FPS value back (`user_fps_cap`) and clear `restore_pending`.
7. Rename the window `Roblox · <username>`.
8. Move on to the next account.

**Stop all** empties the queue and closes every Roblox process Dockie launched. **Stop** closes one account's process.

### FPS cap restore

The goal is that a Roblox started without Dockie always starts with your own value.

- **After each launch:** the queue puts your value back as soon as the window is open (step 6 above).
- **When a Dockie-launched window closes:** Roblox writes its own cap into the file as it exits. Dockie notices the process ended through its once-a-second process check, then writes `user_fps_cap` back.
- **When Dockie starts:** it puts your value back if `restore_pending` is set, or if any PID in `launched` has died since. Dead entries are then dropped.
- **Known limit:** if you change the FPS cap in-game in a *normal* Roblox while a Dockie window is open, Dockie's later restore puts back the older value. Other in-game settings (graphics, volume…) still save over each other across windows. That's how Roblox shares the one file.
- **Values:** check which `FramerateCap` values Roblox accepts by changing it in-game and reading the file. If Roblox only accepts its menu values, the field becomes a dropdown of those. Otherwise it's a number box plus "Unlimited". **Builder's call after checking.**

### Previews, focus and titles

- **Ticker:** once a second, for each running Dockie-launched window:
  - If `IsIconic`, show "minimized".
  - Otherwise capture the window, scale it to about 240 px wide, and send it to the UI as a JPEG data URL through an event.
  - Re-apply the title if Roblox has reset it.
- **Capture method:** start with `PrintWindow(hwnd, PW_RENDERFULLCONTENT)`. Switch to Windows.Graphics.Capture if Roblox comes out black. ExoPanda uses WGC with PrintWindow as a fallback. **Builder's call.**
- **Click a preview:** restore the window if it's minimized, then bring it to the front.

### Freeing RAM from minimized windows (option)

- **When it runs:** only with `trim_minimized` on, and only on Roblox windows Dockie launched or re-attached. Never on a visible window, so nothing stutters while you play.
- **How:**
  - The preview ticker already checks `IsIconic`. Once a window has been minimized for 10 s, Dockie calls `K32EmptyWorkingSet` on its process. The process handle is opened with `PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION`.
  - It repeats every 5 minutes while the window stays minimized, because the running game pulls memory back in.
  - Builder: measure the regrowth and SSD writes, then tune the interval.
- **Turning the setting off** stops future trims. Memory that was already moved comes back on its own as the game uses it.
- **If Windows or Roblox's anti-tamper refuses the handle,** Dockie shows "Roblox blocked freeing RAM" once and switches the setting off.
- **SSD wear:** only memory the game changed since the last trim gets written. That's small next to a modern SSD's write endurance.

### Startup

1. Take the multi-instance lock, or show the warning bar.
2. Check every account in parallel (at most 4 at a time), refreshing names and avatars and marking expired ones.
3. Re-attach windows that are still running from `launched`, and restore the FPS value if needed.

### Import / export

- **Export:** saves the ticked accounts, or all of them if none are ticked, through a save dialog:
  `{dockie_export:1, exported_at, accounts:[{user_id, username, display_name, note, join, fps_cap, cookie}]}`.
  The cookie is **plain text**, as you chose. The dialog warns in one line that the file gives full access to those accounts.
- **Import:** checks the file's format. For each account, it replaces the saved one with the same `user_id` or adds it. Then it runs the startup check on what was imported. Global settings are never exported.
- ⚠️ Roblox may reject a session that suddenly appears from another location. **Test** whether an imported account still works on a second PC or network. If not, it shows "Needs sign-in" there.

### UI (dark, three tabs, no ExoPanda branding)

```
┌ Dockie ─────────────────────────────────────────────────────────┐
│ [!] Roblox is already running — extra windows can't open.  [Close all Roblox] │
│  Accounts   Tools   Settings                    [+ Sign in] [Paste cookie] │
│ ┌────────────────────────────────────────────────────────────┐ │
│ │ Join selected into: [ paste link / id / @user      ] [Go]  │ │
│ │ [Launch selected] [Stop all]          [Import] [Export]     │ │
│ ├────────────────────────────────────────────────────────────┤ │
│ │☐ (av) Jake ✓ @JakeAlt2  ● running  [preview]               │ │
│ │       Join [https://roblox.com/games/920587237…] Adopt Me!  │ │
│ │       FPS [default ▾]  Note [Jake's alt      ]  [Stop] [⋯]  │ │
│ ├────────────────────────────────────────────────────────────┤ │
│ │☐ (av) Sam   @SamMain   ⚠ Needs sign-in  [Sign in again]     │ │
│ └────────────────────────────────────────────────────────────┘ │
│ Tools: running Roblox windows (account / PID) [Focus][Close] … [Close all Roblox] │
│ Settings: default FPS cap [—] · ☐ Free RAM from minimized windows · open data folder · version │
└──────────────────────────────────────────────────────────────────┘
```

`⋯` holds Remove (with a confirm) and Copy username.

### Errors shown on the row

Needs sign-in · Roblox isn't installed · Couldn't get a launch ticket (with Roblox's message) · Window didn't appear within 60 s · Couldn't read this link · Couldn't take the multi-instance lock.

## Open issues

**Unverified (testing decides; may change the design)**

1. **Multi-instance lock:** it's unclear whether `ROBLOX_singletonMutex` alone still works or the `ROBLOX_singletonEvent` is also needed. Any Roblox update can break this, and Dockie's core feature is down until it's fixed.
2. **PlaceLauncher host:** `assetgame.roblox.com` is an old Roblox address. It may need to be `www.roblox.com/Game/PlaceLauncher.ashx` instead.
3. **Which process owns the window:** Roblox may hand off to a child process, so the PID Dockie started might not be the one that owns the window.
4. **FPS values:** check which `FramerateCap` values Roblox accepts. This decides between a number box and a dropdown.
5. **Previews:** if PrintWindow captures Roblox as black, use the heavier Windows.Graphics.Capture method.
6. **Window titles:** Roblox may keep resetting the title. The ticker re-applies it, which may cause a visible flicker.
7. **Imported accounts:** Roblox may sign out a session that is used from another PC or network.
8. **Unofficial web addresses:** the ticket, share-link and PlaceLauncher addresses aren't public APIs. They can change or rate-limit (HTTP 429), and the startup check with many accounts is the most likely to hit a limit.
9. **Freeing RAM:** check how much it saves, how fast memory grows back while the game keeps running, and whether Roblox's anti-tamper allows the call.

**Known limits (accepted)**

10. Closing Dockie stops *new* extra windows. Windows that are already open stay open.
11. An in-game FPS change made in a normal Roblox while a Dockie window is open gets overwritten by the restore. Other in-game settings are shared across all windows.
12. Export files contain plain-text cookies, which give full access to those accounts.

**Outside the code**

13. Roblox doesn't support running several windows. An update may break it, and Roblox could act against it.
14. The exe is unsigned, so friends will see Windows SmartScreen's "Windows protected your PC" warning. Some antivirus tools may flag it, because reading cookies and closing processes looks like what account stealers do. Code signing costs money.
15. The end-to-end test needs the user to sign in to two real accounts.

## Left to the builder

- **Crates:** tauri 2.12, tauri-plugin-dialog, reqwest (rustls), serde/serde_json, `windows` 0.62.2, `image` (for scaling and JPEG), base64, url, tokio.
- **Front end:** plain HTML/CSS/JS from a static `ui/` folder, with no build step. The page's security policy must allow images from `https://*.rbxcdn.com`.
- **Capture method** and the **FPS value set** (see above).
- **Build:** `cargo tauri build --no-bundle` produces `target/release/dockie.exe`. The CLI comes from `cargo install tauri-cli` or npm.
- **Exact styling.** It only needs to be dark and similar in layout to ExoPanda Multi.

## Testing

- **Unit tests:** Join box parsing (every rule plus the ambiguous cases), the FPS restore decisions as a pure state function, export/import round trip and duplicate replacement, and DPAPI round trip.
- **Manual end-to-end:** needs the user to sign in to two real accounts themselves, since the builder never types passwords. Check:
  1. Both accounts launch side by side.
  2. Previews and titles appear.
  3. The FPS override applies to the Dockie window, and a normal Roblox launched afterwards still has 240.
  4. Closing a Dockie window doesn't leave its FPS value in the file.
  5. A private server and a follow join both work.
  6. Export, then import on a clean data folder.
  7. With Roblox open before Dockie, the warning bar appears and Close all works.
  8. With Free RAM on, a minimized window's RAM drops, the game stays connected, and it recovers when restored.
