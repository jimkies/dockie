use crate::app::{AppCtx, is_alive};
use crate::procs::{self, ProcInfo};
use crate::{fps, preview};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
struct Preview {
  user_id: u64,
  minimized: bool,
  image: Option<String>,
}

/// Drops dead launched entries and restores the user's FPS cap; skipped while a launch is in flight.
pub fn reconcile(ctx: &AppCtx, procs: &[ProcInfo]) {
  if ctx.launching.load(Ordering::SeqCst) {
    return;
  }
  let cap = fps::reconcile(&mut ctx.state.lock().unwrap(), &|l| is_alive(l, procs), false);
  if let Some(v) = cap {
    let _ = fps::apply(v);
    let _ = ctx.save_state();
  }
}

pub fn start(app: AppHandle) {
  // a plain thread: captures and trims are blocking calls
  std::thread::spawn(move || {
    let mut minimized_since: HashMap<u32, u64> = HashMap::new();
    let mut last_trim: HashMap<u32, u64> = HashMap::new();
    loop {
      std::thread::sleep(std::time::Duration::from_secs(1));
      tick(&app, &mut minimized_since, &mut last_trim);
    }
  });
}

fn tick(app: &AppHandle, minimized_since: &mut HashMap<u32, u64>, last_trim: &mut HashMap<u32, u64>) {
  let ctx = app.state::<AppCtx>();
  let procs = procs::roblox_processes();
  reconcile(&ctx, &procs);
  let alive: Vec<_> = ctx.state.lock().unwrap().launched.iter().filter(|l| is_alive(l, &procs)).cloned().collect();
  let names: HashMap<u64, String> = ctx.accounts.lock().unwrap().iter().map(|a| (a.user_id, a.username.clone())).collect();
  let mut trim_on = ctx.settings.lock().unwrap().trim_minimized;
  let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
  minimized_since.retain(|p, _| alive.iter().any(|l| l.pid == *p));
  last_trim.retain(|p, _| alive.iter().any(|l| l.pid == *p));

  for l in &alive {
    let Some((_, hwnd)) = procs::main_window(&[l.pid]) else { continue };
    let minimized = procs::is_minimized(hwnd);
    let image = if minimized { None } else { preview::capture_data_url(hwnd).ok() };
    let _ = app.emit("preview", Preview { user_id: l.user_id, minimized, image });
    if let Some(name) = names.get(&l.user_id) {
      let want = format!("Roblox · {name}");
      if procs::title(hwnd) != want {
        procs::set_title(hwnd, &want);
      }
    }
    if !minimized {
      minimized_since.remove(&l.pid);
    } else if trim_on {
      let since = *minimized_since.entry(l.pid).or_insert(now);
      if preview::should_trim(now, Some(since), last_trim.get(&l.pid).copied()) {
        if procs::trim(l.pid).is_ok() {
          last_trim.insert(l.pid, now);
        } else {
          trim_on = false;
          let mut s = ctx.settings.lock().unwrap();
          s.trim_minimized = false;
          let _ = ctx.store.save_settings(&s);
          let _ = app.emit("notice", serde_json::json!({ "message": "Roblox blocked freeing RAM" }));
        }
      }
    }
  }
  let user_ids: Vec<u64> = alive.iter().map(|l| l.user_id).collect();
  let _ = app.emit("running", serde_json::json!({ "user_ids": user_ids }));
}

/// True when a Dockie-launched Roblox window is still open.
pub fn any_alive(ctx: &AppCtx) -> bool {
  let procs = procs::roblox_processes();
  ctx.state.lock().unwrap().launched.iter().any(|l| is_alive(l, &procs))
}
