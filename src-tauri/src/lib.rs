pub mod app;
pub mod commands;
pub mod dpapi;
pub mod executor;
pub mod fps;
pub mod join;
pub mod launcher;
pub mod logfile;
pub mod login;
pub mod preview;
pub mod procs;
pub mod roblox_api;
pub mod store;
pub mod ticker;
pub mod transfer;

use tauri::{Emitter, Manager};

#[tauri::command]
fn quit(app: tauri::AppHandle) {
  app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // two copies would overwrite each other's files; checked before any window exists, and held until exit
  let Ok(_instance) = procs::acquire_lock("Dockie_instanceMutex", "Dockie_instanceEvent") else {
    procs::focus_title("Dockie");
    return;
  };
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
      let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
      let dir = std::path::PathBuf::from(local).join("Dockie");
      logfile::init(&dir);
      logfile::log("Dockie started");
      let ctx = app::AppCtx::new(dir);
      *ctx.lock.lock().unwrap() = procs::take_lock(procs::LOCK_NAMES.0, procs::LOCK_NAMES.1).ok();
      ticker::reconcile(&ctx);
      app.manage(ctx);
      app.manage(launcher::Launcher::start(app.handle().clone()));
      tauri::async_runtime::spawn(app::check_all(app.handle().clone()));
      let h = app.handle().clone();
      tauri::async_runtime::spawn(async move { executor::autostart(&h).await });
      ticker::start(app.handle().clone());
      Ok(())
    })
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" && ticker::any_alive(&window.state::<app::AppCtx>()) {
          api.prevent_close();
          let _ = window.emit("confirm-quit", ());
        }
      }
    })
    .invoke_handler(tauri::generate_handler![
      commands::get_snapshot,
      commands::search_games,
      commands::add_cookie,
      commands::begin_login,
      commands::cancel_login,
      commands::remove_account,
      commands::set_note,
      commands::set_join,
      commands::set_fps_cap,
      commands::set_hide_alt_tab,
      commands::set_settings,
      commands::check_accounts,
      commands::export_accounts,
      commands::import_accounts,
      commands::open_data_folder,
      commands::launch,
      commands::stop,
      commands::stop_all,
      commands::focus,
      commands::focus_pid,
      commands::describe_join,
      commands::roblox_windows,
      commands::close_pid,
      commands::close_all_roblox,
      quit,
    ])
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
