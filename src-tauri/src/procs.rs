use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, FILETIME, HANDLE, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
  CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{K32EmptyWorkingSet, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{
  CreateEventW, CreateMutexW, GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
  PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
  EnumWindows, FindWindowW, GetWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
  SetWindowTextW, ShowWindow, GW_OWNER, SW_RESTORE,
};

#[derive(Clone, Debug)]
pub struct ProcInfo {
  pub pid: u32,
  pub parent: u32,
  pub created: u64, // FILETIME as u64
}

pub const LOCK_NAMES: (&str, &str) = ("ROBLOX_singletonMutex", "ROBLOX_singletonEvent");

fn wide(s: &str) -> Vec<u16> {
  s.encode_utf16().chain(Some(0)).collect()
}

fn hwnd(h: isize) -> HWND {
  HWND(h as *mut _)
}

fn reg_command(root: HKEY) -> Option<String> {
  let sub = wide(r"Software\Classes\roblox-player\shell\open\command");
  // HKCR has no "Software\Classes" prefix.
  let sub = if root == HKEY_CLASSES_ROOT { wide(r"roblox-player\shell\open\command") } else { sub };
  let mut buf = vec![0u16; 1024];
  let mut bytes = (buf.len() * 2) as u32;
  let err = unsafe {
    RegGetValueW(root, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut bytes))
  };
  if err.0 != 0 {
    return None;
  }
  buf.truncate((bytes as usize / 2).saturating_sub(1));
  Some(String::from_utf16_lossy(&buf))
}

/// The exe of a quoted `roblox-player` open command; None when the command isn't quoted.
fn player_exe(cmd: &str) -> Option<Result<PathBuf>> {
  let path = PathBuf::from(cmd.strip_prefix('"')?.split('"').next()?);
  let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
  if !name.eq_ignore_ascii_case("RobloxPlayerBeta.exe") {
    return Some(Err(anyhow!("Roblox is set to open with another launcher ({name}). Dockie needs the official Roblox player.")));
  }
  Some(Ok(path))
}

pub fn roblox_exe() -> Result<PathBuf> {
  for root in [HKEY_CURRENT_USER, HKEY_CLASSES_ROOT] {
    if let Some(r) = reg_command(root).and_then(|c| player_exe(&c)) {
      return r;
    }
  }
  bail!("Roblox isn't installed")
}

fn created(pid: u32) -> u64 {
  unsafe {
    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return 0 };
    let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    let ok = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok();
    let _ = CloseHandle(h);
    if ok { (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64 } else { 0 }
  }
}

pub fn roblox_processes() -> Vec<ProcInfo> {
  let mut out = Vec::new();
  unsafe {
    let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
    let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut more = Process32FirstW(snap, &mut e).is_ok();
    while more {
      let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
      if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case("RobloxPlayerBeta.exe") {
        out.push(ProcInfo { pid: e.th32ProcessID, parent: e.th32ParentProcessID, created: created(e.th32ProcessID) });
      }
      more = Process32NextW(snap, &mut e).is_ok();
    }
    let _ = CloseHandle(snap);
  }
  out
}

pub fn descendants_or_self(root: u32, all: &[ProcInfo]) -> Vec<u32> {
  let mut out = vec![root];
  let mut i = 0;
  while i < out.len() {
    let cur = out[i];
    let kids: Vec<u32> = all.iter().filter(|p| p.parent == cur && !out.contains(&p.pid)).map(|p| p.pid).collect();
    out.extend(kids);
    i += 1;
  }
  out
}

pub fn spawn(exe: &Path, uri: &str) -> Result<u32> {
  Ok(std::process::Command::new(exe).arg(uri).spawn()?.id())
}

pub fn main_window(pids: &[u32]) -> Option<(u32, isize)> {
  struct Ctx<'a> { pids: &'a [u32], found: Option<(u32, isize)> }
  unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lp.0 as *mut Ctx) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    if ctx.pids.contains(&pid)
      && unsafe { IsWindowVisible(h) }.as_bool()
      && unsafe { GetWindow(h, GW_OWNER) }.is_err()
      && !title(h.0 as isize).is_empty()
    {
      ctx.found = Some((pid, h.0 as isize));
      return false.into();
    }
    true.into()
  }
  let mut ctx = Ctx { pids, found: None };
  // Stopping the enumeration early makes EnumWindows return Err; ignore it.
  let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize)) };
  ctx.found
}

pub fn is_minimized(h: isize) -> bool {
  unsafe { IsIconic(hwnd(h)) }.as_bool()
}

pub fn focus(h: isize) {
  unsafe {
    if is_minimized(h) {
      let _ = ShowWindow(hwnd(h), SW_RESTORE);
    }
    let _ = SetForegroundWindow(hwnd(h));
  }
}

/// Brings the top-level window with exactly this title to the front, if there is one.
pub fn focus_title(t: &str) {
  let t = wide(t);
  if let Ok(h) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(t.as_ptr())) } {
    focus(h.0 as isize);
  }
}

pub fn title(h: isize) -> String {
  let mut buf = [0u16; 512];
  let n = unsafe { GetWindowTextW(hwnd(h), &mut buf) };
  String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn set_title(h: isize, t: &str) {
  let t = wide(t);
  let _ = unsafe { SetWindowTextW(hwnd(h), PCWSTR(t.as_ptr())) };
}

pub fn kill(pid: u32) -> Result<()> {
  unsafe {
    let h = OpenProcess(PROCESS_TERMINATE, false, pid)?;
    let r = TerminateProcess(h, 1);
    let _ = CloseHandle(h);
    Ok(r?)
  }
}

/// RAM the process is using right now (its working set), in bytes. None when Windows won't say.
pub fn working_set(pid: u32) -> Option<u64> {
  unsafe {
    // limited access is all this needs, and is what Roblox allows
    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut c = PROCESS_MEMORY_COUNTERS::default();
    let ok = K32GetProcessMemoryInfo(h, &mut c, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32).as_bool();
    let _ = CloseHandle(h);
    ok.then_some(c.WorkingSetSize as u64)
  }
}

/// Now, in FILETIME units (100 ns since 1601), to compare with `ProcInfo::created`.
pub fn filetime_now() -> u64 {
  let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  (unix.as_nanos() / 100) as u64 + 116_444_736_000_000_000
}

pub fn uptime_secs(created: u64, now: u64) -> u64 {
  now.saturating_sub(created) / 10_000_000
}

pub fn trim(pid: u32) -> Result<()> {
  unsafe {
    let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
    let r = K32EmptyWorkingSet(h).ok();
    let _ = CloseHandle(h);
    Ok(r?)
  }
}

pub struct MultiLock(HANDLE, HANDLE);

// kernel handles may be closed from any thread
unsafe impl Send for MultiLock {}

impl Drop for MultiLock {
  fn drop(&mut self) {
    unsafe {
      let _ = CloseHandle(self.0);
      let _ = CloseHandle(self.1);
    }
  }
}

pub fn acquire_lock(mutex_name: &str, event_name: &str) -> Result<MultiLock> {
  let (m, e) = (wide(mutex_name), wide(event_name));
  unsafe {
    let mutex = CreateMutexW(None, true, PCWSTR(m.as_ptr()))?;
    if GetLastError() == ERROR_ALREADY_EXISTS {
      let _ = CloseHandle(mutex);
      return Err(anyhow!("another process already holds {mutex_name}"));
    }
    match CreateEventW(None, true, false, PCWSTR(e.as_ptr())) {
      Ok(event) => Ok(MultiLock(mutex, event)),
      Err(err) => {
        let _ = CloseHandle(mutex);
        Err(err.into())
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn p(pid: u32, parent: u32) -> ProcInfo { ProcInfo { pid, parent, created: 0 } }

  #[test]
  fn uptime_counts_whole_seconds() {
    let c = 133_000_000_000_000_000u64;
    assert_eq!(uptime_secs(c, c + 90 * 10_000_000 + 9_999_999), 90);
    assert_eq!(uptime_secs(c + 5, c), 0); // clock skew never underflows
  }

  #[test]
  fn reads_own_working_set() {
    assert!(working_set(std::process::id()).unwrap() > 0);
    assert!(filetime_now() > 133_000_000_000_000_000); // after 2022
  }

  #[test]
  fn descendants_includes_children_of_dead_parent() {
    let all = [p(10, 1), p(11, 10), p(12, 11), p(20, 1)];
    let mut d = descendants_or_self(10, &all);
    d.sort();
    assert_eq!(d, vec![10, 11, 12]);
  }

  #[test]
  fn lock_acquire_and_release() {
    let id = std::process::id();
    let (m, e) = (format!("Dockie_test_m_{id}"), format!("Dockie_test_e_{id}"));
    let first = acquire_lock(&m, &e).unwrap();
    assert!(acquire_lock(&m, &e).is_err());
    drop(first);
    assert!(acquire_lock(&m, &e).is_ok());
  }

  #[test]
  fn player_exe_must_be_the_official_player() {
    let ok = player_exe(r#""C:\R\Versions\version-1\RobloxPlayerBeta.exe" "%1""#).unwrap().unwrap();
    assert!(ok.ends_with("RobloxPlayerBeta.exe"));
    let err = player_exe(r#""C:\B\Bloxstrap.exe" -player "%1""#).unwrap().unwrap_err().to_string();
    assert_eq!(err, "Roblox is set to open with another launcher (Bloxstrap.exe). Dockie needs the official Roblox player.");
    assert!(player_exe("unquoted.exe %1").is_none());
  }

  // Needs Roblox installed.
  #[test]
  #[ignore]
  fn roblox_exe_found_on_this_pc() {
    assert!(roblox_exe().unwrap().ends_with("RobloxPlayerBeta.exe"));
  }
}
