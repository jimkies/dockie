use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows::core::{BOOL, PCWSTR};
use windows::Wdk::Foundation::{NtQueryObject, OBJECT_INFORMATION_CLASS};
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Win32::Foundation::{
  CloseHandle, DuplicateHandle, GetLastError, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, ERROR_ALREADY_EXISTS, ERROR_INVALID_HANDLE, FILETIME,
  HANDLE, HWND, LPARAM, STATUS_INFO_LENGTH_MISMATCH, UNICODE_STRING,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
  CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{K32EmptyWorkingSet, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{
  GetCurrentProcess, GetCurrentProcessId, PROCESS_DUP_HANDLE,
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
  // The roblox-player link can go missing (another launcher's uninstaller removes it) while Roblox
  // itself is still installed, so look in its install folders too.
  let dirs: Vec<PathBuf> = ["LOCALAPPDATA", "ProgramFiles(x86)", "ProgramFiles"]
    .iter()
    .filter_map(|k| std::env::var_os(k).map(|d| PathBuf::from(d).join("Roblox").join("Versions")))
    .collect();
  newest_player(&dirs).ok_or_else(|| anyhow!("Roblox isn't installed"))
}

/// The most recently updated RobloxPlayerBeta.exe in any version folder under these directories.
fn newest_player(dirs: &[PathBuf]) -> Option<PathBuf> {
  dirs
    .iter()
    .filter_map(|d| std::fs::read_dir(d).ok())
    .flatten()
    .flatten()
    .map(|v| v.path().join("RobloxPlayerBeta.exe"))
    .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
    .max_by_key(|(t, _)| *t)
    .map(|(_, p)| p)
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

/// Time running, or None when the start time is unknown (`created` is 0 when Windows won't say).
pub fn uptime_of(created: u64, now: u64) -> Option<u64> {
  (created != 0).then(|| uptime_secs(created, now))
}

pub fn trim(pid: u32) -> Result<()> {
  unsafe {
    let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
    let r = K32EmptyWorkingSet(h).ok();
    let _ = CloseHandle(h);
    Ok(r?)
  }
}

/// The mutex, and the event when we created it (None when another tool already holds the event's name).
pub struct MultiLock(HANDLE, Option<HANDLE>);

// kernel handles may be closed from any thread
unsafe impl Send for MultiLock {}

impl Drop for MultiLock {
  fn drop(&mut self) {
    unsafe {
      let _ = CloseHandle(self.0);
      if let Some(event) = self.1 {
        let _ = CloseHandle(event);
      }
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
      Ok(event) => Ok(MultiLock(mutex, Some(event))),
      // Another multi-instance tool (ExoPanda does this) holds the event's name as a different kind
      // of object. Roblox can't create its singleton event then either, so extra windows already
      // work: keep our mutex and carry on instead of refusing to launch.
      Err(err) if err.code() == ERROR_INVALID_HANDLE.to_hresult() => Ok(MultiLock(mutex, None)),
      Err(err) => {
        let _ = CloseHandle(mutex);
        Err(err.into())
      }
    }
  }
}

/// Takes Roblox's multi-instance lock. If another program holds it (Roblox opened before Dockie,
/// or another multi-instance tool), its handles are closed first so Dockie can take it over.
pub fn take_lock(mutex_name: &str, event_name: &str) -> Result<MultiLock> {
  acquire_lock(mutex_name, event_name).or_else(|_| {
    close_foreign_handles(&[mutex_name, event_name]);
    acquire_lock(mutex_name, event_name)
  })
}

/// One row of NtQuerySystemInformation(SystemExtendedHandleInformation).
#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
  object: usize,
  pid: usize,
  handle: usize,
  access: u32,
  creator: u16,
  kind: u16,
  attributes: u32,
  reserved: u32,
}

/// Every open handle on the system.
fn system_handles() -> Vec<HandleEntry> {
  let mut len: usize = 4 << 20;
  loop {
    let mut buf = vec![0usize; len / 8]; // usize-aligned for the entries
    let mut ret = 0u32;
    // SAFETY: buf is writable for its full byte length; the call writes at most that much.
    let st = unsafe { NtQuerySystemInformation(SYSTEM_INFORMATION_CLASS(64), buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut ret) };
    if st == STATUS_INFO_LENGTH_MISMATCH {
      len = (ret as usize).max(len * 2) + (1 << 16); // the table grows while we ask
      continue;
    }
    if st.0 < 0 {
      return Vec::new();
    }
    let room = (buf.len() * 8 - 16) / std::mem::size_of::<HandleEntry>();
    let count = buf[0].min(room);
    // SAFETY: entries follow the two-usize header and `count` fits inside buf.
    return unsafe { std::slice::from_raw_parts(buf.as_ptr().add(2).cast::<HandleEntry>(), count) }.to_vec();
  }
}

/// The kernel name of one of our handles, like `\Sessions\1\BaseNamedObjects\ROBLOX_singletonMutex`.
fn object_name(h: HANDLE) -> Option<String> {
  let mut buf = vec![0usize; 512]; // 4 KB, aligned for UNICODE_STRING
  let mut ret = 0u32;
  // SAFETY: buf is writable for its full byte length.
  let st = unsafe { NtQueryObject(Some(h), OBJECT_INFORMATION_CLASS(1), Some(buf.as_mut_ptr().cast()), (buf.len() * 8) as u32, Some(&mut ret)) };
  if st.0 < 0 {
    return None;
  }
  // SAFETY: on success buf starts with a UNICODE_STRING whose buffer points inside buf.
  let s = unsafe { &*buf.as_ptr().cast::<UNICODE_STRING>() };
  if s.Buffer.is_null() {
    return None;
  }
  Some(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(s.Buffer.0, s.Length as usize / 2) }))
}

/// Closes, inside other processes, every handle to a mutex or event with one of these names.
/// Programs Dockie may not open (running as administrator) are skipped. Only mutex and event
/// handles are looked at: asking the name of some other kinds, like pipes, can hang.
pub fn close_foreign_handles(names: &[&str]) -> usize {
  let suffixes: Vec<String> = names.iter().map(|n| format!("\\{n}")).collect();
  unsafe {
    let me = GetCurrentProcessId() as usize;
    // learn the type numbers of "mutex" and "event" from two throwaway objects of our own
    let probes = [CreateMutexW(None, false, PCWSTR::null()), CreateEventW(None, true, false, PCWSTR::null())];
    let all = system_handles();
    let kinds: Vec<u16> = probes
      .iter()
      .flatten()
      .filter_map(|p| all.iter().find(|e| e.pid == me && e.handle == p.0 as usize).map(|e| e.kind))
      .collect();
    for p in probes.iter().flatten() {
      let _ = CloseHandle(*p);
    }
    let mut procs: HashMap<usize, Option<HANDLE>> = HashMap::new();
    let mut closed = 0;
    for e in all.iter().filter(|e| e.pid != me && kinds.contains(&e.kind)) {
      let Some(ph) = *procs.entry(e.pid).or_insert_with(|| OpenProcess(PROCESS_DUP_HANDLE, false, e.pid as u32).ok()) else { continue };
      let theirs = HANDLE(e.handle as *mut _);
      let mut ours = HANDLE::default();
      if DuplicateHandle(ph, theirs, GetCurrentProcess(), &mut ours, 0, false, DUPLICATE_SAME_ACCESS).is_err() {
        continue;
      }
      let name = object_name(ours);
      let _ = CloseHandle(ours);
      if name.is_some_and(|n| suffixes.iter().any(|s| n.ends_with(s.as_str())))
        && DuplicateHandle(ph, theirs, HANDLE::default(), std::ptr::null_mut(), 0, false, DUPLICATE_CLOSE_SOURCE).is_ok()
      {
        closed += 1;
      }
    }
    for ph in procs.values().flatten() {
      let _ = CloseHandle(*ph);
    }
    closed
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn p(pid: u32, parent: u32) -> ProcInfo { ProcInfo { pid, parent, created: 0 } }

  // Names unique to this test run, so tests never touch Roblox's real lock.
  fn names(tag: &str) -> (String, String) {
    let id = std::process::id();
    (format!("DockieTest_{tag}_mutex_{id}"), format!("DockieTest_{tag}_event_{id}"))
  }

  #[test]
  fn lock_works_beside_another_multi_instance_tool() {
    // ExoPanda keeps a *mutex* under the event's name; Roblox can't make its singleton event then,
    // so multi-instance already works and Dockie should carry on rather than fail.
    let (m, e) = names("exopanda");
    let w = wide(&e);
    let squatter = unsafe { CreateMutexW(None, false, PCWSTR(w.as_ptr())) }.expect("stand-in mutex");
    let lock = acquire_lock(&m, &e);
    unsafe { let _ = CloseHandle(squatter); }
    assert!(lock.is_ok(), "{:?}", lock.err());
  }

  #[test]
  fn finds_newest_player_without_the_registry_link() {
    let root = std::env::temp_dir().join(format!("dockie_versions_{}", std::process::id()));
    let versions = root.join("Versions");
    let make = |v: &str, secs: u64| {
      let d = versions.join(v);
      std::fs::create_dir_all(&d).unwrap();
      let f = std::fs::File::create(d.join("RobloxPlayerBeta.exe")).unwrap();
      f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)).unwrap();
    };
    make("version-old", 1_700_000_000);
    make("version-new", 1_800_000_000);
    std::fs::create_dir_all(versions.join("version-empty")).unwrap(); // no player in it
    let found = newest_player(&[versions.clone(), root.join("missing")]);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(found, Some(versions.join("version-new").join("RobloxPlayerBeta.exe")));
  }

  #[test]
  fn uptime_counts_whole_seconds() {
    let c = 133_000_000_000_000_000u64;
    assert_eq!(uptime_secs(c, c + 90 * 10_000_000 + 9_999_999), 90);
    assert_eq!(uptime_secs(c + 5, c), 0); // clock skew never underflows
  }

  #[test]
  fn unknown_start_time_has_no_uptime() {
    let c = 133_000_000_000_000_000u64;
    assert_eq!(uptime_of(0, c), None); // created() is 0 when Windows won't say
    assert_eq!(uptime_of(c, c + 90 * 10_000_000), Some(90));
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
