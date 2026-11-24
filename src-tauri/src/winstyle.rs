//! Window styles Dockie changes with plain Win32 calls: executor windows leave Alt+Tab and the
//! taskbar, and an account's Roblox window can leave Alt+Tab while keeping its taskbar button.
//! Every change is recorded in state.json so it can be put back exactly, even after a crash.
use crate::app::AppCtx;
use crate::logfile::log;
use crate::procs;
use crate::store::Styled;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, GetLastError, HWND, LPARAM, SetLastError, WIN32_ERROR};
use windows::Win32::UI::WindowsAndMessaging::{
  EnumWindows, GWL_EXSTYLE, GW_OWNER, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SW_HIDE, SW_SHOW,
  SW_SHOWNA, SetWindowLongPtrW, ShowWindow, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

const TOOL: isize = WS_EX_TOOLWINDOW.0 as isize;
const APP: isize = WS_EX_APPWINDOW.0 as isize;

/// Set by the shell-restart listener: every account window needs its taskbar button again.
pub static READD_TABS: AtomicBool = AtomicBool::new(false);
// the executor PID whose windows Windows wouldn't let us change (it runs as admin)
static BLOCKED: Mutex<Option<u32>> = Mutex::new(None);

pub(crate) fn hwnd(h: isize) -> HWND {
  HWND(h as *mut _)
}

/// Out of Alt+Tab: a tool window, and never an app window (APPWINDOW would force it back in).
pub fn hidden_style(style: isize) -> isize {
  (style | TOOL) & !APP
}

fn is_hidden_style(style: isize) -> bool {
  style & TOOL != 0 && style & APP == 0
}

pub fn exstyle(h: isize) -> isize {
  unsafe { GetWindowLongPtrW(hwnd(h), GWL_EXSTYLE) }
}

/// SetWindowLongPtr returns the old value, which may be 0, so the error has to be read separately.
fn set_exstyle(h: isize, v: isize) -> Result<(), WIN32_ERROR> {
  unsafe {
    SetLastError(WIN32_ERROR(0));
    let old = SetWindowLongPtrW(hwnd(h), GWL_EXSTYLE, v);
    let err = GetLastError();
    if old == 0 && err.0 != 0 { Err(err) } else { Ok(()) }
  }
}

pub fn owner_pid(h: isize) -> u32 {
  let mut pid = 0;
  unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
  pid
}

/// The window still exists and still belongs to the same process (HWNDs get reused).
pub fn same_window(h: isize, pid: u32) -> bool {
  unsafe { IsWindow(Some(hwnd(h))) }.as_bool() && owner_pid(h) == pid
}

pub fn is_visible(h: isize) -> bool {
  unsafe { IsWindowVisible(hwnd(h)) }.as_bool()
}

/// The shell only re-reads the style when the window is shown again.
fn reshow(h: isize) {
  unsafe {
    let _ = ShowWindow(hwnd(h), SW_HIDE);
    let _ = ShowWindow(hwnd(h), SW_SHOWNA);
  }
}

/// Puts a taskbar button back for a tool window (it keeps the window out of Alt+Tab).
fn add_tab(h: isize) {
  use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
  use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
  thread_local!(static COM: () = { let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }; });
  COM.with(|_| ());
  let r = unsafe { CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER).and_then(|t| t.HrInit().and_then(|_| t.AddTab(hwnd(h)))) };
  if let Err(e) = r {
    log(format!("winstyle: AddTab failed for window {h}: {e}"));
  }
}

/// Visible, unowned top-level windows of a process: the ones the shell shows. Hidden helper
/// windows are never touched, so restoring can't show something that was meant to stay hidden.
pub fn top_windows(pid: u32) -> Vec<isize> {
  struct Ctx {
    pid: u32,
    out: Vec<isize>,
  }
  unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the &mut Ctx passed below, alive for the whole EnumWindows call.
    let ctx = unsafe { &mut *(lp.0 as *mut Ctx) };
    let h = h.0 as isize;
    if owner_pid(h) == ctx.pid && is_visible(h) && unsafe { GetWindow(hwnd(h), GW_OWNER) }.is_err() {
      ctx.out.push(h);
    }
    true.into()
  }
  let mut ctx = Ctx { pid, out: vec![] };
  let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize)) };
  ctx.out
}

/// Gives the window the hidden style (recording its original first) or re-applies it if the app reset it.
/// Err(ACCESS_DENIED) when the window's process runs as admin.
fn ensure(list: &mut Vec<Styled>, h: isize, pid: u32, user_id: Option<u64>) -> Result<bool, WIN32_ERROR> {
  let cur = exstyle(h);
  let known = list.iter().position(|s| s.hwnd == h && s.pid == pid);
  if let Some(i) = known {
    if is_hidden_style(cur) {
      return Ok(false);
    }
    set_exstyle(h, hidden_style(cur))?;
    if !list[i].tray_hidden {
      reshow(h);
    }
    log(format!("winstyle: style was reset, applied again to window {h} (pid {pid})"));
  } else {
    set_exstyle(h, hidden_style(cur))?;
    list.push(Styled { hwnd: h, pid, original: cur, user_id, tray_hidden: false });
    reshow(h);
    log(format!("winstyle: hid window {h} (pid {pid}, {}) from Alt+Tab", user_id.map_or("executor".into(), |u| format!("user {u}"))));
  }
  if user_id.is_some() {
    add_tab(h);
  }
  Ok(true)
}

/// Puts one window back exactly as it was, showing it if Dockie hid it to the tray.
fn restore(s: &Styled) {
  if !same_window(s.hwnd, s.pid) {
    return;
  }
  let _ = set_exstyle(s.hwnd, s.original);
  if s.tray_hidden {
    unsafe {
      let _ = ShowWindow(hwnd(s.hwnd), SW_SHOW);
    }
  } else if is_visible(s.hwnd) {
    reshow(s.hwnd);
  }
  log(format!("winstyle: restored window {} (pid {})", s.hwnd, s.pid));
}

/// Set once Dockie starts shutting down, so a tick in flight can't style a window after the restore.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Restores every window Dockie changed: on exit, and at start after a crash. `from_panic` only
/// tries the lock, since the panicking thread may be the one holding it.
pub fn restore_all(ctx: &AppCtx, from_panic: bool) {
  let list = {
    let mut st = if from_panic {
      match ctx.state.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
      }
    } else {
      ctx.state.lock().unwrap_or_else(|e| e.into_inner())
    };
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    std::mem::take(&mut st.styled)
  };
  if list.is_empty() {
    return;
  }
  list.iter().for_each(restore);
  let _ = ctx.save_state();
}

/// Lets ticks style windows again (startup's crash restore runs before the ticker exists).
pub fn resume() {
  SHUTTING_DOWN.store(false, Ordering::SeqCst);
}

/// One pass, from the ticker: applies, re-applies and undoes styles so windows match the settings.
/// Returns the executor PID being managed, for the tray and the minimize hook. Works on a copy of
/// the list: changing another process's window waits on that process, and a hung Roblox must not
/// hold the state lock the UI needs.
pub fn tick(app: &AppHandle, alive: &[crate::store::LaunchedProc]) -> Option<u32> {
  if SHUTTING_DOWN.load(Ordering::SeqCst) {
    return None;
  }
  let ctx = app.state::<AppCtx>();
  let s = ctx.settings.lock().unwrap().clone();
  let want: HashMap<u64, bool> = ctx.accounts.lock().unwrap().iter().map(|a| (a.user_id, a.hide_alt_tab)).collect();
  let running = crate::executor::running_pid(&s.executor_path);
  let blocked = {
    let mut b = BLOCKED.lock().unwrap();
    if b.is_some() && *b != running {
      *b = None; // that elevated copy has exited
    }
    *b
  };
  let exe_pid = running.filter(|p| s.manage_executor_windows && Some(*p) != blocked);
  let wants = |u: u64| want.get(&u).copied().unwrap_or(false);

  let mut list = ctx.state.lock().unwrap().styled.clone();
  list.retain(|w| same_window(w.hwnd, w.pid));
  // undo what's no longer wanted
  let (keep, undo): (Vec<Styled>, Vec<Styled>) = list.into_iter().partition(|w| match w.user_id {
    None => Some(w.pid) == exe_pid,
    Some(u) => wants(u) && alive.iter().any(|l| l.pid == w.pid),
  });
  undo.iter().for_each(restore);
  let mut list = keep;

  if let Some(pid) = exe_pid {
    for h in top_windows(pid) {
      if ensure(&mut list, h, pid, None) == Err(ERROR_ACCESS_DENIED) {
        *BLOCKED.lock().unwrap() = Some(pid);
        log(format!("winstyle: executor pid {pid} runs as administrator; its windows can't be changed"));
        let _ = app.emit("notice", serde_json::json!({ "message": "The executor is running as administrator, so Dockie can't hide its windows. Close it and let Dockie start it." }));
        break;
      }
    }
  }
  // there's no way to ask whether a taskbar button is still there, so re-add every 10 s too (a no-op when it is)
  static TICKS: AtomicU32 = AtomicU32::new(0);
  let readd = READD_TABS.swap(false, Ordering::SeqCst) | (TICKS.fetch_add(1, Ordering::Relaxed) % 10 == 0);
  for l in alive.iter().filter(|l| wants(l.user_id)) {
    let Some((pid, h)) = procs::main_window(&[l.pid]) else { continue };
    if ensure(&mut list, h, pid, Some(l.user_id)) == Ok(false) && readd {
      add_tab(h);
    }
  }

  let mut st = ctx.state.lock().unwrap();
  if SHUTTING_DOWN.load(Ordering::SeqCst) {
    // restore_all already ran on the old list; put back what this pass changed too
    drop(st);
    list.iter().for_each(restore);
    return None;
  }
  // the tray and the minimize hook may have hidden or shown windows meanwhile
  for w in &mut list {
    if let Some(o) = st.styled.iter().find(|o| o.hwnd == w.hwnd && o.pid == w.pid) {
      w.tray_hidden = o.tray_hidden;
    }
  }
  let changed = st.styled != list;
  st.styled = list;
  drop(st);
  if changed {
    let _ = ctx.save_state();
  }
  exe_pid
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hidden_style_is_tool_and_never_app_window() {
    let normal = 0x0001_0100; // WS_EX_CONTROLPARENT | WS_EX_WINDOWEDGE
    assert_eq!(hidden_style(normal), normal | TOOL);
    assert_eq!(hidden_style(normal | APP), normal | TOOL); // APPWINDOW would put it back in Alt+Tab
    assert!(is_hidden_style(hidden_style(normal | APP)));
    assert!(!is_hidden_style(normal) && !is_hidden_style(TOOL | APP));
  }

  #[test]
  fn restoring_a_window_that_is_gone_does_nothing() {
    // a stale HWND from a crashed session must never be touched
    restore(&Styled { hwnd: 0x7fff_fff0, pid: 1, original: 0, user_id: None, tray_hidden: true });
    assert!(!same_window(0x7fff_fff0, 1));
  }
}
