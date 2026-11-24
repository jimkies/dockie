//! Starts the executor in the background. Dockie only starts its process: it never reads or
//! changes the executor's files, and never touches Roblox.
use crate::logfile::log;
use crate::procs;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

/// The exe file name in a path, which is also its process name.
pub fn exe_name(path: &str) -> Option<String> {
  let p = Path::new(path.trim());
  p.file_name().filter(|_| !path.trim().is_empty()).map(|n| n.to_string_lossy().into_owned())
}

/// The PID of the running executor, if there is one.
pub fn running_pid(path: &str) -> Option<u32> {
  procs::processes_named(&exe_name(path)?).first().map(|p| p.pid)
}

/// Starts the executor (minimized, un-elevated) unless it already runs, then waits up to 10 s for it.
pub async fn ensure_running(path: &str) -> Result<u32, String> {
  let path = path.trim();
  let name = exe_name(path).ok_or("Choose the executor's app in Settings first")?;
  if let Some(p) = procs::processes_named(&name).first() {
    return Ok(p.pid);
  }
  if !Path::new(path).is_file() {
    log(format!("executor: not found at {path}"));
    return Err(format!("The executor isn't at {path}. Check it in Settings."));
  }
  spawn_unelevated(Path::new(path)).map_err(|e| {
    log(format!("executor: start failed: {e}"));
    format!("Couldn't start the executor: {}", e.message())
  })?;
  for _ in 0..40 {
    tokio::time::sleep(Duration::from_millis(250)).await;
    if let Some(p) = procs::processes_named(&name).first() {
      log(format!("executor: started {name} pid {}", p.pid));
      return Ok(p.pid);
    }
  }
  log("executor: process didn't appear within 10 s");
  Err("The executor didn't start within 10 s".into())
}

pub const SCRIPT_NAME: &str = "dockie_antiafk.lua";

/// The autoexec script with Dockie's WebSocket port filled in.
pub fn script(port: u16) -> String {
  include_str!("dockie_antiafk.lua").replace("__PORT__", &port.to_string())
}

/// Writes dockie_antiafk.lua into the autoexec folder when its content differs. Touches no other
/// file there. Ok(true) when it wrote.
pub fn write_script(dir: &str, port: u16) -> Result<bool, String> {
  let dir = Path::new(dir.trim());
  if dir.as_os_str().is_empty() {
    return Ok(false);
  }
  if !dir.is_dir() {
    return Err(format!("The autoexec folder {} doesn't exist. Check it in Settings.", dir.display()));
  }
  let path = dir.join(SCRIPT_NAME);
  let want = script(port);
  if std::fs::read_to_string(&path).is_ok_and(|have| have == want) {
    return Ok(false);
  }
  std::fs::write(&path, want).map(|_| true).map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

/// Puts the current script in place, logging the outcome and telling the UI about a failure.
pub fn deploy_script(app: &tauri::AppHandle) {
  use tauri::{Emitter, Manager};
  let s = app.state::<crate::app::AppCtx>().settings.lock().unwrap().clone();
  match write_script(&s.autoexec_dir, s.ws_port) {
    Ok(true) => log(format!("autoexec: wrote {SCRIPT_NAME} (port {})", s.ws_port)),
    Ok(false) => {}
    Err(m) => {
      log(format!("autoexec: {m}"));
      let _ = app.emit("notice", serde_json::json!({ "message": m }));
    }
  }
}

/// When "Start executor with Dockie" is on: starts it, and tells the UI if that failed.
pub async fn autostart(app: &tauri::AppHandle) {
  use tauri::{Emitter, Manager};
  let s = app.state::<crate::app::AppCtx>().settings.lock().unwrap().clone();
  if !s.executor_autostart || s.executor_path.trim().is_empty() {
    return;
  }
  if let Err(m) = ensure_running(&s.executor_path).await {
    let _ = app.emit("notice", serde_json::json!({ "message": m }));
  }
}

/// The environment block CreateProcessW wants: sorted `KEY=value\0` entries, ending with an extra `\0`.
fn env_block(vars: impl Iterator<Item = (OsString, OsString)>, key: &str, value: &str) -> Vec<u16> {
  let mut all: Vec<(OsString, OsString)> = vars.filter(|(k, _)| !k.to_string_lossy().eq_ignore_ascii_case(key)).collect();
  all.push((key.into(), value.into()));
  all.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
  let mut out = Vec::new();
  for (k, v) in all {
    out.extend(k.encode_wide());
    out.push('=' as u16);
    out.extend(v.encode_wide());
    out.push(0);
  }
  out.push(0);
  out
}

fn wide(p: &std::ffi::OsStr) -> Vec<u16> {
  p.encode_wide().chain(Some(0)).collect()
}

/// Its manifest asks for admin; RunAsInvoker starts it as a normal app, so there's no UAC prompt
/// and Dockie (not admin) may change its windows. Run from its own folder, as its files expect.
fn spawn_unelevated(exe: &Path) -> windows::core::Result<()> {
  use windows::Win32::Foundation::CloseHandle;
  use windows::Win32::System::Threading::{CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW};
  use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
  use windows::core::{PCWSTR, PWSTR};
  let env = env_block(std::env::vars_os(), "__COMPAT_LAYER", "RunAsInvoker");
  let app = wide(exe.as_os_str());
  let mut cmd = wide(&OsString::from(format!("\"{}\"", exe.display())));
  let dir = wide(exe.parent().unwrap_or(Path::new(".")).as_os_str());
  let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, dwFlags: STARTF_USESHOWWINDOW, wShowWindow: SW_SHOWMINNOACTIVE.0 as u16, ..Default::default() };
  let mut pi = PROCESS_INFORMATION::default();
  // SAFETY: every buffer is NUL-terminated and outlives the call; cmd is mutable as CreateProcessW requires.
  unsafe {
    CreateProcessW(PCWSTR(app.as_ptr()), Some(PWSTR(cmd.as_mut_ptr())), None, None, false, CREATE_UNICODE_ENVIRONMENT, Some(env.as_ptr().cast()), PCWSTR(dir.as_ptr()), &si, &mut pi)?;
    let _ = CloseHandle(pi.hThread);
    let _ = CloseHandle(pi.hProcess);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exe_name_is_the_file_name() {
    assert_eq!(exe_name(r" C:\x\Potassium.exe ").as_deref(), Some("Potassium.exe"));
    assert_eq!(exe_name("  "), None);
  }

  #[test]
  fn env_block_adds_the_flag_once_sorted_and_terminated() {
    let vars = [("Path", "p"), ("__compat_layer", "old"), ("A", "1")].map(|(k, v)| (OsString::from(k), OsString::from(v)));
    let block = String::from_utf16(&env_block(vars.into_iter(), "__COMPAT_LAYER", "RunAsInvoker")).unwrap();
    assert_eq!(block, "A=1\0Path=p\0__COMPAT_LAYER=RunAsInvoker\0\0");
  }

  #[test]
  fn script_is_written_only_when_changed_and_nothing_else_is_touched() {
    let dir = std::env::temp_dir().join(format!("dockie_autoexec_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("antiafk.luau"), "mine").unwrap();
    let d = dir.to_string_lossy().into_owned();
    let first = write_script(&d, 47613);
    let again = write_script(&d, 47613);
    let new_port = write_script(&d, 50000);
    let body = std::fs::read_to_string(dir.join(SCRIPT_NAME)).unwrap();
    let other = std::fs::read_to_string(dir.join("antiafk.luau")).unwrap();
    let count = std::fs::read_dir(&dir).unwrap().count();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((first, again, new_port), (Ok(true), Ok(false), Ok(true)));
    assert!(body.contains("ws://127.0.0.1:50000") && !body.contains("__PORT__"));
    assert_eq!((other.as_str(), count), ("mine", 2));
    assert_eq!(write_script("", 1), Ok(false)); // not set up yet
    assert!(write_script(r"C:
o\suchutoexec\dir", 1).is_err());
  }

  #[test]
  fn missing_exe_is_an_error_not_a_crash() {
    let e = tauri::async_runtime::block_on(ensure_running(r"C:\definitely\not\here\NoSuchExecutor123.exe")).unwrap_err();
    assert!(e.contains("isn't at"), "{e}");
    assert!(tauri::async_runtime::block_on(ensure_running("")).is_err());
  }
}
