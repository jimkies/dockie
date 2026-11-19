use crate::app::{AddResult, AppCtx, Snapshot, add_account, check_all, now_ms};
use crate::store::{Account, Settings};
use crate::launcher::{self, Job, Launcher, RobloxWindow};
use crate::{dpapi, join, login, transfer};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
  err.to_string()
}

/// Applies `f` to a copy of the account list, saves it, then swaps it in and tells the UI.
fn change<T>(app: &AppHandle, f: impl FnOnce(&mut Vec<Account>) -> Res<T>) -> Res<T> {
  let ctx = app.state::<AppCtx>();
  let out = {
    let mut list = ctx.accounts.lock().unwrap();
    let mut next = list.clone();
    let out = f(&mut next)?;
    ctx.save_list(&next).map_err(e)?;
    *list = next;
    out
  };
  let _ = app.emit("accounts-changed", ());
  Ok(out)
}

fn edit(app: &AppHandle, user_id: u64, f: impl FnOnce(&mut Account)) -> Res<()> {
  change(app, |l| Ok(f(l.iter_mut().find(|a| a.user_id == user_id).ok_or("Account not found")?)))
}

#[tauri::command]
pub fn get_snapshot(ctx: State<AppCtx>) -> Snapshot {
  ctx.snapshot()
}

#[tauri::command]
pub async fn add_cookie(app: AppHandle, cookie: String) -> Res<AddResult> {
  add_account(&app, &cookie, None).await
}

#[tauri::command]
pub async fn begin_login(app: AppHandle, refresh_user_id: Option<u64>) -> Res<AddResult> {
  login::sign_in(app, refresh_user_id).await
}

#[tauri::command]
pub fn cancel_login(app: AppHandle) {
  if let Some(w) = app.get_webview_window("login") {
    let _ = w.close();
  }
}

#[tauri::command]
pub fn remove_account(app: AppHandle, user_id: u64) -> Res<()> {
  change(&app, |l| Ok(l.retain(|a| a.user_id != user_id)))
}

#[tauri::command]
pub fn set_note(app: AppHandle, user_id: u64, note: String) -> Res<()> {
  edit(&app, user_id, |a| a.note = note)
}

#[tauri::command]
pub fn set_join(app: AppHandle, user_id: u64, join: String) -> Res<()> {
  edit(&app, user_id, |a| a.join = join)
}

#[tauri::command]
pub fn set_fps_cap(app: AppHandle, user_id: u64, cap: Option<i32>) -> Res<()> {
  edit(&app, user_id, |a| a.fps_cap = cap)
}

#[tauri::command]
pub fn set_settings(ctx: State<AppCtx>, settings: Settings) -> Res<()> {
  ctx.store.save_settings(&settings).map_err(e)?;
  *ctx.settings.lock().unwrap() = settings;
  Ok(())
}

#[tauri::command]
pub async fn check_accounts(app: AppHandle) {
  check_all(app).await
}

#[tauri::command]
pub async fn export_accounts(app: AppHandle, user_ids: Vec<u64>) -> Res<()> {
  let list = app.state::<AppCtx>().accounts.lock().unwrap().clone();
  let Some(path) = app
    .dialog()
    .file()
    .set_title("This file gives full access to these accounts. Only share it with people you trust.")
    .set_file_name("dockie-accounts.json")
    .add_filter("JSON", &["json"])
    .blocking_save_file()
  else {
    return Ok(());
  };
  let file = transfer::to_export(&list, &user_ids, now_ms(), &dpapi::unprotect).map_err(e)?;
  std::fs::write(path.into_path().map_err(e)?, serde_json::to_vec_pretty(&file).map_err(e)?).map_err(e)
}

#[tauri::command]
pub async fn import_accounts(app: AppHandle) -> Res<(usize, usize)> {
  let Some(path) = app.dialog().file().add_filter("JSON", &["json"]).blocking_pick_file() else { return Ok((0, 0)) };
  let items = transfer::parse_import(&std::fs::read_to_string(path.into_path().map_err(e)?).map_err(e)?).map_err(e)?;
  // change() imports into a clone and only swaps it in once saved
  let counts = change(&app, |l| transfer::import_into(l, items, now_ms(), &dpapi::protect).map_err(e))?;
  tauri::async_runtime::spawn(check_all(app.clone()));
  Ok(counts)
}

#[tauri::command]
pub fn open_data_folder(ctx: State<AppCtx>) -> Res<()> {
  std::process::Command::new("explorer").arg(&ctx.store.dir).spawn().map(|_| ()).map_err(e)
}

#[tauri::command]
pub fn launch(ctx: State<AppCtx>, launcher: State<Launcher>, user_ids: Vec<u64>, target: Option<String>) -> Res<()> {
  if let Some(t) = &target {
    join::parse_join(t)?;
  }
  let (to_launch, to_focus) = launcher::split_running(&user_ids, &launcher::running_ids(&ctx));
  for id in to_focus {
    let _ = launcher::focus_account(&ctx, id);
  }
  launcher.enqueue(to_launch.into_iter().map(|user_id| Job { user_id, target: target.clone() }).collect());
  Ok(())
}

#[tauri::command]
pub fn stop(ctx: State<AppCtx>, user_id: u64) {
  launcher::stop(&ctx, Some(user_id))
}

#[tauri::command]
pub fn stop_all(ctx: State<AppCtx>, launcher: State<Launcher>) {
  launcher.clear();
  launcher::stop(&ctx, None)
}

#[tauri::command]
pub fn focus(ctx: State<AppCtx>, user_id: u64) -> Res<()> {
  launcher::focus_account(&ctx, user_id)
}

#[tauri::command]
pub async fn describe_join(app: AppHandle, user_id: u64, text: String) -> String {
  let ctx = app.state::<AppCtx>();
  let target = match join::parse_join(&text) {
    Ok(t) => t,
    Err(m) => return m,
  };
  let cached = ctx.labels.lock().unwrap().get(&text).cloned();
  if let Some(l) = cached {
    return l;
  }
  let cookie = ctx.cookie(user_id).unwrap_or_default();
  match ctx.api.describe(&cookie, &target).await {
    Ok(l) => {
      ctx.labels.lock().unwrap().insert(text, l.clone());
      l
    }
    Err(m) => m.to_string(),
  }
}

#[tauri::command]
pub fn roblox_windows(ctx: State<AppCtx>) -> Vec<RobloxWindow> {
  launcher::windows(&ctx)
}

#[tauri::command]
pub fn close_pid(pid: u32) -> Res<()> {
  crate::procs::kill(pid).map_err(e)
}

#[tauri::command]
pub async fn close_all_roblox(app: AppHandle) -> bool {
  launcher::close_all(&app.state::<AppCtx>()).await
}
