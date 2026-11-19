use crate::app::{AppCtx, is_alive, now_ms};
use crate::join::{launch_uri, parse_join};
use crate::procs::{self, LOCK_NAMES, descendants_or_self, main_window, roblox_processes};
use crate::roblox_api::ApiError;
use crate::store::{AccountStatus, LaunchedProc};
use crate::fps;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

pub struct Job {
  pub user_id: u64,
  pub target: Option<String>, // Some = "Join selected into…" override
}

pub struct Launcher {
  app: AppHandle,
  queue: Arc<Mutex<VecDeque<Job>>>,
  notify: Arc<Notify>,
}

#[derive(Serialize, Clone)]
struct Status {
  user_id: u64,
  state: &'static str,
  message: Option<String>,
}

#[derive(Serialize)]
pub struct RobloxWindow {
  pub pid: u32,
  pub title: String,
  pub user_id: Option<u64>,
}

fn emit(app: &AppHandle, user_id: u64, state: &'static str, message: Option<String>) {
  let _ = app.emit("launch-status", Status { user_id, state, message });
}

impl Launcher {
  pub fn start(app: AppHandle) -> Launcher {
    let l = Launcher { app: app.clone(), queue: Arc::default(), notify: Arc::default() };
    let (queue, notify) = (l.queue.clone(), l.notify.clone());
    tauri::async_runtime::spawn(async move {
      loop {
        let job = queue.lock().unwrap().pop_front();
        let Some(job) = job else {
          notify.notified().await;
          continue;
        };
        let ctx = app.state::<AppCtx>();
        let exists = ctx.accounts.lock().unwrap().iter().any(|a| a.user_id == job.user_id);
        if !exists {
          continue;
        }
        // queued twice, or already running: focus instead of launching again
        if running_ids(&ctx).contains(&job.user_id) {
          let _ = focus_account(&ctx, job.user_id);
          emit(&app, job.user_id, "running", None);
          continue;
        }
        ctx.launching.store(true, Ordering::SeqCst);
        emit(&app, job.user_id, "launching", None);
        match run(&app, &job).await {
          Ok(()) => emit(&app, job.user_id, "running", None),
          Err(m) => emit(&app, job.user_id, "error", Some(m)),
        }
        ctx.launching.store(false, Ordering::SeqCst);
      }
    });
    l
  }

  pub fn enqueue(&self, jobs: Vec<Job>) {
    for j in &jobs {
      emit(&self.app, j.user_id, "queued", None);
    }
    self.queue.lock().unwrap().extend(jobs);
    self.notify.notify_one();
  }

  pub fn clear(&self) {
    self.queue.lock().unwrap().clear();
  }
}

/// (to_launch, to_focus), order kept.
pub fn split_running(ids: &[u64], running: &HashSet<u64>) -> (Vec<u64>, Vec<u64>) {
  ids.iter().partition(|id| !running.contains(id))
}

fn ensure_lock(ctx: &AppCtx) -> Result<(), String> {
  let mut lock = ctx.lock.lock().unwrap();
  if lock.is_none() {
    *lock = Some(procs::acquire_lock(LOCK_NAMES.0, LOCK_NAMES.1).map_err(|_| "Couldn't take the multi-instance lock")?);
  }
  Ok(())
}

fn restore_fps(ctx: &AppCtx) {
  let v = fps::after_launch(&mut ctx.state.lock().unwrap());
  let _ = ctx.save_state();
  if let Some(v) = v {
    let _ = fps::apply(v);
  }
}

async fn run(app: &AppHandle, job: &Job) -> Result<(), String> {
  let ctx = app.state::<AppCtx>();
  let (username, acc_cap, join) = {
    let list = ctx.accounts.lock().unwrap();
    let a = list.iter().find(|a| a.user_id == job.user_id).ok_or("Account not found")?;
    (a.username.clone(), a.fps_cap, a.join.clone())
  };
  ensure_lock(&ctx)?;
  let cookie = ctx.cookie(job.user_id)?;
  let target = parse_join(job.target.as_deref().unwrap_or(&join))?;
  let exe = procs::roblox_exe().map_err(|e| e.to_string())?;
  let ticket_err = |e: ApiError| match e {
    ApiError::NeedsSignIn => {
      if let Some(a) = ctx.accounts.lock().unwrap().iter_mut().find(|a| a.user_id == job.user_id) {
        a.status = AccountStatus::NeedsSignIn;
      }
      let _ = ctx.save_accounts();
      let _ = app.emit("accounts-changed", ());
      e.to_string()
    }
    ApiError::Other(m) => format!("Couldn't get a launch ticket: {m}"),
  };
  let resolved = ctx.api.resolve(&cookie, &target).await.map_err(ticket_err)?;
  let ticket = ctx.api.auth_ticket(&cookie).await.map_err(ticket_err)?;
  let uri = launch_uri(&ticket, &resolved, now_ms(), now_ms());

  let default_cap = ctx.settings.lock().unwrap().fps_cap_default;
  let cap = acc_cap.or(default_cap);
  let mut capped = false;
  if let Some(cap) = cap {
    // while a Dockie window is open the file may hold its cap, so trust the stored user value
    let user_cap = if running_ids(&ctx).is_empty() {
      std::fs::read_to_string(fps::settings_path()).ok().and_then(|x| fps::read_cap(&x))
    } else {
      ctx.state.lock().unwrap().user_fps_cap
    };
    // no user value to put back means we must not write a cap
    if user_cap.is_some() {
      fps::before_launch(&mut ctx.state.lock().unwrap(), user_cap);
      let _ = ctx.save_state();
      let _ = fps::apply(cap); // Ok(false) just means no file to edit
      capped = true; // restore even if the write failed midway
    }
  }
  let r = spawn_and_wait(&ctx, job.user_id, &username, &exe, &uri).await;
  if capped {
    restore_fps(&ctx);
  }
  r
}

async fn spawn_and_wait(ctx: &AppCtx, user_id: u64, username: &str, exe: &std::path::Path, uri: &str) -> Result<(), String> {
  let pid = procs::spawn(exe, uri).map_err(|e| e.to_string())?;
  // tracked from the start so Stop / Stop all reach it; on timeout it stays tracked (may be a self-update)
  let created = |all: &[procs::ProcInfo], pid| all.iter().find(|p| p.pid == pid).map_or(0, |p| p.created);
  ctx.state.lock().unwrap().launched.push(LaunchedProc { pid, started_at: created(&roblox_processes(), pid), user_id });
  let _ = ctx.save_state();
  for _ in 0..120 {
    tokio::time::sleep(Duration::from_millis(500)).await;
    let all = roblox_processes();
    let tree = descendants_or_self(pid, &all);
    if let Some((owner, hwnd)) = main_window(&tree) {
      {
        let mut st = ctx.state.lock().unwrap();
        st.launched.retain(|l| !(l.pid == pid && l.user_id == user_id));
        st.launched.push(LaunchedProc { pid: owner, started_at: created(&all, owner), user_id });
      }
      let _ = ctx.save_state();
      procs::set_title(hwnd, &format!("Roblox · {username}"));
      return Ok(());
    }
    if !all.iter().any(|p| tree.contains(&p.pid)) {
      return Err("Roblox closed before its window appeared".into());
    }
  }
  Err("Window didn't appear within 60 s".into())
}

pub fn running_ids(ctx: &AppCtx) -> HashSet<u64> {
  let procs = roblox_processes();
  ctx.state.lock().unwrap().launched.iter().filter(|l| is_alive(l, &procs)).map(|l| l.user_id).collect()
}

fn alive_pids(ctx: &AppCtx, user_id: Option<u64>) -> Vec<u32> {
  let procs = roblox_processes();
  let state = ctx.state.lock().unwrap();
  state.launched.iter().filter(|l| user_id.is_none_or(|u| l.user_id == u) && is_alive(l, &procs)).map(|l| l.pid).collect()
}

pub fn focus_account(ctx: &AppCtx, user_id: u64) -> Result<(), String> {
  let all = roblox_processes();
  let pids = alive_pids(ctx, Some(user_id));
  let (_, hwnd) = pids.iter().find_map(|p| main_window(&descendants_or_self(*p, &all))).ok_or("No window to focus")?;
  procs::focus(hwnd);
  Ok(())
}

/// One account's Dockie windows, or all of them with None.
pub fn stop(ctx: &AppCtx, user_id: Option<u64>) {
  for pid in alive_pids(ctx, user_id) {
    let _ = procs::kill(pid);
  }
}

pub fn windows(ctx: &AppCtx) -> Vec<RobloxWindow> {
  let all = roblox_processes();
  let state = ctx.state.lock().unwrap();
  all
    .iter()
    .filter_map(|p| {
      let (pid, hwnd) = main_window(&[p.pid])?;
      let user_id = state.launched.iter().find(|l| l.pid == pid && is_alive(l, &all)).map(|l| l.user_id);
      Some(RobloxWindow { pid, title: procs::title(hwnd), user_id })
    })
    .collect()
}

/// Kills every Roblox process, then takes the lock if we don't hold it. Returns lock_ok.
pub async fn close_all(ctx: &AppCtx) -> bool {
  for p in roblox_processes() {
    let _ = procs::kill(p.pid);
  }
  // killed processes take a moment to release the mutex
  for _ in 0..12 {
    if ensure_lock(ctx).is_ok() {
      return true;
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
  }
  false
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn split_keeps_order() {
    let running: HashSet<u64> = [2].into();
    assert_eq!(split_running(&[1, 2, 3], &running), (vec![1, 3], vec![2]));
  }
}
