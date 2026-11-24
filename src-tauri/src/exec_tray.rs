//! The executor's tray icon, its minimize-to-tray hook, and the shell-restart listener.
//! A "hooks" thread owns a hidden window (it hears TaskbarCreated when Explorer restarts) and an
//! out-of-context WinEvent hook scoped to the executor's PID (it hears minimizes).
use crate::app::AppCtx;
use crate::logfile::log;
use crate::winstyle::{self, hwnd, is_visible, owner_pid};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_SYSTEM_MINIMIZESTART, GetForegroundWindow, GetMessageW, IsIconic, MSG,
  OBJID_WINDOW, PostMessageW, PostThreadMessageW, RegisterClassW, RegisterWindowMessageW, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow,
  ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_CLOSE, WM_QUIT,
  WNDCLASSW,
};
use windows::core::w;

const TRAY_ID: &str = "executor";
const WM_WATCH_PID: u32 = WM_APP + 1;

static APP: OnceLock<AppHandle> = OnceLock::new();
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static WATCHED: AtomicU32 = AtomicU32::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static TRAY_SHOWN: AtomicBool = AtomicBool::new(false);

/// Starts the hooks thread and builds the (hidden) tray icon.
pub fn start(app: &AppHandle) -> tauri::Result<()> {
  let _ = APP.set(app.clone());
  std::thread::spawn(hooks_thread);
  let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
  let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
  let exit = MenuItem::with_id(app, "exit", "Exit executor", true, None::<&str>)?;
  let mut b = TrayIconBuilder::with_id(TRAY_ID)
    .tooltip("Executor (hidden by Dockie)")
    .menu(&Menu::with_items(app, &[&show, &hide, &exit])?)
    .show_menu_on_left_click(false)
    .on_menu_event(|app, e| match e.id().as_ref() {
      "show" => show_windows(app),
      "hide" => hide_windows(app),
      "exit" => exit_executor(app),
      _ => {}
    })
    .on_tray_icon_event(|tray, e| {
      if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
        toggle(tray.app_handle());
      }
    });
  if let Some(icon) = app.default_window_icon() {
    b = b.icon(icon.clone());
  }
  b.build(app)?.set_visible(false)?;
  Ok(())
}

/// From the ticker: shows the tray icon while a managed executor runs, and points the minimize hook at it.
pub fn sync(app: &AppHandle, exe_pid: Option<u32>, exe_name: &str) {
  let pid = exe_pid.unwrap_or(0);
  if WATCHED.swap(pid, Ordering::SeqCst) != pid {
    let t = HOOK_THREAD.load(Ordering::SeqCst);
    if t != 0 {
      let _ = unsafe { PostThreadMessageW(t, WM_WATCH_PID, WPARAM(pid as usize), LPARAM(0)) };
    }
  }
  let show = exe_pid.is_some();
  if TRAY_SHOWN.swap(show, Ordering::SeqCst) != show
    && let Some(t) = app.tray_by_id(TRAY_ID)
  {
    let _ = t.set_tooltip(Some(format!("{exe_name} (hidden by Dockie)")));
    let _ = t.set_visible(show);
  }
}

/// On exit: unhooks, removes the tray icon.
pub fn stop(app: &AppHandle) {
  let t = HOOK_THREAD.load(Ordering::SeqCst);
  if t != 0 {
    let _ = unsafe { PostThreadMessageW(t, WM_QUIT, WPARAM(0), LPARAM(0)) };
  }
  let _ = app.remove_tray_by_id(TRAY_ID);
}

fn executor_windows(app: &AppHandle) -> Vec<isize> {
  let ctx = app.state::<AppCtx>();
  let st = ctx.state.lock().unwrap();
  st.styled.iter().filter(|s| s.user_id.is_none() && winstyle::same_window(s.hwnd, s.pid)).map(|s| s.hwnd).collect()
}

fn set_tray_hidden(app: &AppHandle, h: Option<isize>, hidden: bool) {
  let ctx = app.state::<AppCtx>();
  let mut st = ctx.state.lock().unwrap();
  for s in st.styled.iter_mut().filter(|s| s.user_id.is_none() && h.is_none_or(|h| s.hwnd == h)) {
    s.tray_hidden = hidden;
  }
  drop(st);
  let _ = ctx.save_state();
}

fn show_windows(app: &AppHandle) {
  let wins = executor_windows(app);
  set_tray_hidden(app, None, false);
  for h in &wins {
    unsafe {
      let _ = ShowWindow(hwnd(*h), if IsIconic(hwnd(*h)).as_bool() { SW_RESTORE } else { SW_SHOW });
    }
  }
  if let Some(h) = wins.first() {
    let _ = unsafe { SetForegroundWindow(hwnd(*h)) };
  }
}

fn hide_windows(app: &AppHandle) {
  for h in executor_windows(app) {
    let _ = unsafe { ShowWindow(hwnd(h), SW_HIDE) };
  }
  set_tray_hidden(app, None, true);
}

/// In front: hide it. Hidden, minimized or behind other windows: bring it up.
fn toggle(app: &AppHandle) {
  let wins = executor_windows(app);
  let fg = unsafe { GetForegroundWindow() }.0 as isize;
  let in_front = wins.iter().any(|h| *h == fg && is_visible(*h) && !unsafe { IsIconic(hwnd(*h)) }.as_bool());
  if in_front { hide_windows(app) } else { show_windows(app) }
}

/// Asks the executor to close; if it's still running 3 s later, ends it.
fn exit_executor(app: &AppHandle) {
  let path = app.state::<AppCtx>().settings.lock().unwrap().executor_path.clone();
  let Some(pid) = crate::executor::running_pid(&path) else { return };
  log(format!("tray: exiting executor pid {pid}"));
  for h in executor_windows(app).into_iter().chain(winstyle::top_windows(pid)) {
    let _ = unsafe { PostMessageW(Some(hwnd(h)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
  }
  std::thread::spawn(move || {
    std::thread::sleep(Duration::from_secs(3));
    if crate::executor::running_pid(&path) == Some(pid) {
      let _ = crate::procs::kill(pid);
    }
  });
}

unsafe extern "system" fn on_minimize(_: HWINEVENTHOOK, _: u32, h: HWND, id_object: i32, _: i32, _: u32, _: u32) {
  if id_object != OBJID_WINDOW.0 {
    return;
  }
  let h = h.0 as isize;
  // never unwind into Windows
  let _ = std::panic::catch_unwind(|| {
    let Some(app) = APP.get() else { return };
    if owner_pid(h) != WATCHED.load(Ordering::SeqCst) || !executor_windows(app).contains(&h) {
      return;
    }
    let _ = unsafe { ShowWindow(hwnd(h), SW_HIDE) };
    set_tray_hidden(app, Some(h), true);
    log(format!("tray: executor window {h} minimized to the tray"));
  });
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::SeqCst) {
    winstyle::READD_TABS.store(true, Ordering::SeqCst);
    log("shell restarted: re-adding taskbar buttons");
  }
  unsafe { DefWindowProcW(h, msg, wp, lp) }
}

fn hooks_thread() {
  unsafe {
    HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
    TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
    let class = w!("DockieShellListener");
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), lpszClassName: class, ..Default::default() });
    // a hidden top-level window: message-only windows don't get the TaskbarCreated broadcast
    let win = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None).ok();
    let mut hook: Option<HWINEVENTHOOK> = None;
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
      if msg.hwnd.is_invalid() && msg.message == WM_WATCH_PID {
        if let Some(old) = hook.take() {
          let _ = UnhookWinEvent(old);
        }
        let pid = msg.wParam.0 as u32;
        if pid != 0 {
          let hk = SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZESTART, None, Some(on_minimize), pid, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
          hook = (!hk.is_invalid()).then_some(hk);
          log(format!("tray: watching executor pid {pid} for minimize"));
        }
        continue;
      }
      let _ = TranslateMessage(&msg);
      DispatchMessageW(&msg);
    }
    if let Some(old) = hook {
      let _ = UnhookWinEvent(old);
    }
    if let Some(w) = win {
      let _ = DestroyWindow(w);
    }
  }
}
