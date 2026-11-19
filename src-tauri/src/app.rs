use crate::dpapi;
use crate::procs::{MultiLock, ProcInfo, roblox_processes};
use crate::roblox_api::{Api, ApiError, UserInfo, normalize_cookie};
use crate::store::{Account, AccountStatus, LaunchedProc, Settings, State, Store, Upsert, upsert};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppCtx {
  pub store: Store,
  pub api: Api,
  pub accounts: Mutex<Vec<Account>>,
  pub settings: Mutex<Settings>,
  pub state: Mutex<State>,
  pub lock: Mutex<Option<MultiLock>>,
  pub launching: AtomicBool,
  pub labels: Mutex<HashMap<String, String>>,
  // set when accounts.json was unreadable; we must not overwrite it
  pub load_err: Option<String>,
}

impl AppCtx {
  pub fn new(dir: std::path::PathBuf) -> AppCtx {
    let store = Store::new(dir);
    let (accounts, load_err) = match store.load_accounts() {
      Ok(a) => (a, None),
      Err(e) => (vec![], Some(format!("accounts.json could not be read: {e}"))),
    };
    let (settings, state) = (store.load_settings(), store.load_state());
    AppCtx {
      store,
      api: Api::new(),
      accounts: Mutex::new(accounts),
      settings: Mutex::new(settings),
      state: Mutex::new(state),
      lock: Mutex::new(None),
      launching: AtomicBool::new(false),
      labels: Mutex::new(HashMap::new()),
      load_err,
    }
  }

  /// Saves the given list; callers already holding the accounts guard pass it in.
  pub fn save_list(&self, list: &[Account]) -> anyhow::Result<()> {
    if let Some(e) = &self.load_err {
      anyhow::bail!("{e}");
    }
    self.store.save_accounts(list)
  }

  pub fn save_accounts(&self) -> anyhow::Result<()> {
    self.save_list(&self.accounts.lock().unwrap())
  }

  pub fn save_state(&self) -> anyhow::Result<()> {
    self.store.save_state(&self.state.lock().unwrap())
  }

  pub fn cookie(&self, user_id: u64) -> Result<String, String> {
    let enc = self.accounts.lock().unwrap().iter().find(|a| a.user_id == user_id).map(|a| a.cookie_dpapi.clone()).ok_or("Account not found")?;
    dpapi::unprotect(&enc).map_err(|_| "Sign-in expired".to_string())
  }

  pub fn snapshot(&self) -> Snapshot {
    let procs = roblox_processes();
    let state = self.state.lock().unwrap();
    let accounts = self
      .accounts
      .lock()
      .unwrap()
      .iter()
      .map(|a| AccountView {
        user_id: a.user_id,
        username: a.username.clone(),
        display_name: a.display_name.clone(),
        avatar_url: a.avatar_url.clone(),
        added_at: a.added_at,
        note: a.note.clone(),
        join: a.join.clone(),
        fps_cap: a.fps_cap,
        status: a.status,
        running: state.launched.iter().any(|l| l.user_id == a.user_id && is_alive(l, &procs)),
      })
      .collect();
    Snapshot { accounts, settings: self.settings.lock().unwrap().clone(), lock_ok: self.lock.lock().unwrap().is_some(), roblox_running: !procs.is_empty(), load_error: self.load_err.clone() }
  }
}

pub fn now_ms() -> u64 {
  std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn is_alive(lp: &LaunchedProc, procs: &[ProcInfo]) -> bool {
  procs.iter().any(|p| p.pid == lp.pid && p.created == lp.started_at)
}

#[derive(Serialize)]
pub struct AccountView {
  pub user_id: u64,
  pub username: String,
  pub display_name: String,
  pub avatar_url: Option<String>,
  pub added_at: u64,
  pub note: String,
  pub join: String,
  pub fps_cap: Option<i32>,
  pub status: AccountStatus,
  pub running: bool,
}

#[derive(Serialize)]
pub struct Snapshot {
  pub accounts: Vec<AccountView>,
  pub settings: Settings,
  pub lock_ok: bool,
  pub roblox_running: bool,
  pub load_error: Option<String>,
}

#[derive(Serialize)]
pub struct AddResult {
  pub user_id: u64,
  pub username: String,
  pub replaced: bool,
  pub was_other_account: bool,
}

pub fn apply_check(acc: &mut Account, r: Result<UserInfo, ApiError>) {
  match r {
    Ok(u) => {
      acc.username = u.name;
      acc.display_name = u.display_name;
      acc.status = AccountStatus::Ok;
    }
    Err(ApiError::NeedsSignIn) => acc.status = AccountStatus::NeedsSignIn,
    Err(ApiError::Other(_)) => {}
  }
}

/// Validates a cookie and stores the account; `refresh` is the account the user meant to sign in as.
pub async fn add_account(app: &AppHandle, cookie: &str, refresh: Option<u64>) -> Result<AddResult, String> {
  let ctx = app.state::<AppCtx>();
  let cookie = normalize_cookie(cookie);
  let user = ctx.api.authenticated_user(&cookie).await.map_err(|e| e.to_string())?;
  let avatar_url = ctx.api.avatar_url(user.id).await.ok().flatten();
  let acc = Account {
    user_id: user.id,
    username: user.name.clone(),
    display_name: user.display_name,
    avatar_url,
    added_at: now_ms(),
    note: String::new(),
    join: String::new(),
    fps_cap: None,
    cookie_dpapi: dpapi::protect(&cookie).map_err(|e| e.to_string())?,
    status: AccountStatus::Ok,
  };
  let replaced = {
    let mut list = ctx.accounts.lock().unwrap();
    let mut next = list.clone();
    let r = upsert(&mut next, acc) == Upsert::Replaced;
    ctx.save_list(&next).map_err(|e| e.to_string())?;
    *list = next;
    r
  };
  let _ = app.emit("accounts-changed", ());
  Ok(AddResult { user_id: user.id, username: user.name, replaced, was_other_account: refresh.is_some_and(|r| r != user.id) })
}

/// Re-checks every account, at most 4 at a time.
pub async fn check_all(app: AppHandle) {
  let ctx = app.state::<AppCtx>();
  let todo: Vec<(u64, String)> = ctx.accounts.lock().unwrap().iter().map(|a| (a.user_id, a.cookie_dpapi.clone())).collect();
  let sem = Arc::new(tokio::sync::Semaphore::new(4));
  let tasks: Vec<_> = todo
    .into_iter()
    .map(|(id, enc)| {
      let (app, sem) = (app.clone(), sem.clone());
      tauri::async_runtime::spawn(async move {
        let _permit = sem.acquire().await;
        let api = &app.state::<AppCtx>().api;
        let r = match dpapi::unprotect(&enc) {
          Err(_) => Err(ApiError::NeedsSignIn),
          Ok(c) => api.authenticated_user(&c).await,
        };
        let avatar = if r.is_ok() { api.avatar_url(id).await.ok().flatten() } else { None };
        (id, r, avatar)
      })
    })
    .collect();
  let mut results = vec![];
  for t in tasks {
    if let Ok(x) = t.await {
      results.push(x);
    }
  }
  {
    let mut list = ctx.accounts.lock().unwrap();
    for (id, r, avatar) in results {
      if let Some(a) = list.iter_mut().find(|a| a.user_id == id) {
        if avatar.is_some() {
          a.avatar_url = avatar;
        }
        apply_check(a, r);
      }
    }
    let _ = ctx.save_list(&list);
  }
  let _ = app.emit("accounts-changed", ());
}

#[cfg(test)]
mod tests {
  use super::*;

  fn acc() -> Account {
    Account { user_id: 1, username: "old".into(), display_name: "Old".into(), avatar_url: None, added_at: 0, note: String::new(), join: String::new(), fps_cap: None, cookie_dpapi: String::new(), status: AccountStatus::Unchecked }
  }

  #[test]
  fn check_marks_needs_sign_in() {
    let mut a = acc();
    apply_check(&mut a, Err(ApiError::Other("net".into())));
    assert_eq!(a.status, AccountStatus::Unchecked);
    apply_check(&mut a, Err(ApiError::NeedsSignIn));
    assert_eq!(a.status, AccountStatus::NeedsSignIn);
    apply_check(&mut a, Ok(UserInfo { id: 1, name: "new".into(), display_name: "New".into() }));
    assert_eq!((a.status, a.username.as_str(), a.display_name.as_str()), (AccountStatus::Ok, "new", "New"));
  }
}
