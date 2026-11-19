use crate::store::{write_atomic, LaunchedProc, State};
use std::path::PathBuf;

const OPEN: &str = "<int name=\"FramerateCap\">";

pub fn settings_path() -> PathBuf {
  PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("Roblox").join("GlobalBasicSettings_13.xml")
}

/// Byte range of the element's value text.
fn value_span(xml: &str) -> Option<(usize, usize)> {
  let a = xml.find(OPEN)? + OPEN.len();
  Some((a, a + xml[a..].find("</int>")?))
}

pub fn read_cap(xml: &str) -> Option<i32> {
  let (a, b) = value_span(xml)?;
  xml[a..b].trim().parse().ok()
}

pub fn write_cap(xml: &str, cap: i32) -> Option<String> {
  let (a, b) = value_span(xml)?;
  Some(format!("{}{}{}", &xml[..a], cap, &xml[b..]))
}

pub fn before_launch(state: &mut State, file_cap: Option<i32>) {
  if !state.restore_pending {
    state.user_fps_cap = file_cap;
  }
  state.restore_pending = true;
}

pub fn after_launch(state: &mut State) -> Option<i32> {
  state.restore_pending = false;
  state.user_fps_cap
}

pub fn reconcile(state: &mut State, alive: &dyn Fn(&LaunchedProc) -> bool, launching: bool) -> Option<i32> {
  let before = state.launched.len();
  state.launched.retain(alive);
  let due = before != state.launched.len() || (state.restore_pending && !launching);
  if due {
    state.restore_pending = false;
  }
  // with no Dockie window left the file is the user's own again, so forget the stored value
  let v = if state.launched.is_empty() && !launching { state.user_fps_cap.take() } else { state.user_fps_cap };
  if due { v } else { None }
}

/// Ok(false) when the file or the FramerateCap element is missing; nothing is written then.
pub fn apply(cap: i32) -> anyhow::Result<bool> {
  let path = settings_path();
  let Ok(xml) = std::fs::read_to_string(&path) else { return Ok(false) };
  let Some(new) = write_cap(&xml, cap) else { return Ok(false) };
  write_atomic(&path, new.as_bytes())?;
  Ok(true)
}

#[cfg(test)]
mod tests {
  use super::*;

  const X: &str = "<Settings>\n\t\t\t<int name=\"FramerateCap\">240</int>\n<int name=\"Other\">1</int></Settings>";

  fn proc(pid: u32) -> LaunchedProc {
    LaunchedProc { pid, started_at: 0, user_id: 0 }
  }

  #[test]
  fn read_and_write_cap() {
    assert_eq!(read_cap(X), Some(240));
    assert_eq!(read_cap("<Settings/>"), None);
    assert_eq!(write_cap(X, 30).unwrap(), X.replace("240", "30"));
    assert_eq!(write_cap("<Settings/>", 30), None);
  }

  #[test]
  fn before_launch_remembers_user_value() {
    let mut s = State::default();
    before_launch(&mut s, Some(240));
    assert_eq!(s.user_fps_cap, Some(240));
    assert!(s.restore_pending);
  }

  #[test]
  fn second_launch_keeps_first_value() {
    let mut s = State::default();
    before_launch(&mut s, Some(240));
    before_launch(&mut s, Some(30));
    assert_eq!(s.user_fps_cap, Some(240));
  }

  #[test]
  fn after_launch_restores() {
    let mut s = State::default();
    before_launch(&mut s, Some(240));
    assert_eq!(after_launch(&mut s), Some(240));
    assert!(!s.restore_pending);
  }

  #[test]
  fn reconcile_dead_window_restores() {
    let mut s = State { user_fps_cap: Some(240), launched: vec![proc(1), proc(2)], ..Default::default() };
    assert_eq!(reconcile(&mut s, &|p| p.pid == 2, false), Some(240));
    assert_eq!(s.launched, vec![proc(2)]);
    assert_eq!(s.user_fps_cap, Some(240));
  }

  #[test]
  fn reconcile_last_window_restores_once_then_forgets() {
    let mut s = State { user_fps_cap: Some(240), launched: vec![proc(1)], ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| false, false), Some(240));
    assert_eq!(s.user_fps_cap, None);
    assert_eq!(reconcile(&mut s, &|_| false, false), None);
    // a later uncapped Dockie window closing must not write the old value
    s.launched.push(proc(2));
    assert_eq!(reconcile(&mut s, &|_| false, false), None);
  }

  #[test]
  fn reconcile_no_window_nothing_due_forgets_silently() {
    let mut s = State { user_fps_cap: Some(240), ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| true, false), None);
    assert_eq!(s.user_fps_cap, None);
  }

  #[test]
  fn reconcile_all_alive_does_nothing() {
    let mut s = State { user_fps_cap: Some(240), launched: vec![proc(1), proc(2)], ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| true, false), None);
    assert_eq!(s.launched, vec![proc(1), proc(2)]);
  }

  #[test]
  fn reconcile_startup_pending_restores() {
    let mut s = State { user_fps_cap: Some(240), restore_pending: true, ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| true, false), Some(240));
    assert!(!s.restore_pending);
  }

  #[test]
  fn reconcile_while_launching_ignores_pending() {
    let mut s = State { user_fps_cap: Some(240), restore_pending: true, ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| true, true), None);
    assert_eq!(s.user_fps_cap, Some(240));
  }

  #[test]
  fn reconcile_never_written_is_none() {
    let mut s = State { restore_pending: true, launched: vec![proc(1)], ..Default::default() };
    assert_eq!(reconcile(&mut s, &|_| false, false), None);
  }
}
