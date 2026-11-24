//! Dockie's log: one line per event in %LOCALAPPDATA%\Dockie\dockie.log, started over past 1 MB.
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static PATH: OnceLock<PathBuf> = OnceLock::new();
static WRITE: Mutex<()> = Mutex::new(());
const MAX: u64 = 1 << 20;

pub fn init(dir: &std::path::Path) {
  let _ = PATH.set(dir.join("dockie.log"));
}

pub fn log(msg: impl AsRef<str>) {
  let Some(p) = PATH.get() else { return };
  let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
  let fresh = std::fs::metadata(p).is_ok_and(|m| m.len() > MAX);
  let f = std::fs::OpenOptions::new().create(true).append(!fresh).write(true).truncate(fresh).open(p);
  if let Ok(mut f) = f {
    let _ = writeln!(f, "{} {}", crate::app::now_ms(), msg.as_ref());
  }
}
