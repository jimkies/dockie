pub mod app;
pub mod attach;
pub mod commands;
pub mod dpapi;
pub mod exec_tray;
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
pub mod winstyle;

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
      // windows Dockie changed before a crash go back to normal first
      winstyle::restore_all(&ctx, false);
      winstyle::resume();
      ticker::reconcile(&ctx);
      app.manage(ctx);
      app.manage(launcher::Launcher::start(app.handle().clone()));
      tauri::async_runtime::spawn(app::check_all(app.handle().clone()));
      executor::deploy_script(app.handle());
      let port = app.state::<app::AppCtx>().settings.lock().unwrap().ws_port;
      if let Err(m) = app.state::<app::AppCtx>().attach.listen(port) {
        let h = app.handle().clone();
        // the window's listener isn't up yet during setup
        tauri::async_runtime::spawn(async move {
          tokio::time::sleep(std::time::Duration::from_secs(2)).await;
          let _ = h.emit("notice", serde_json::json!({ "message": m }));
        });
      }
      let h = app.handle().clone();
      tauri::async_runtime::spawn(async move { executor::autostart(&h).await });
      exec_tray::start(app.handle())?;
      ticker::start(app.handle().clone());
      // a panic on the main thread ends Dockie: put windows back first (the ticker catches its own panics)
      let h = app.handle().clone();
      let default_hook = std::panic::take_hook();
      std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name() == Some("main") {
          winstyle::restore_all(&h.state::<app::AppCtx>(), true);
        }
        default_hook(info);
      }));
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
      commands::pick_path,
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
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app, e| {
      if let tauri::RunEvent::Exit = e {
        logfile::log("Dockie exiting: restoring windows");
        winstyle::restore_all(&app.state::<app::AppCtx>(), false);
        exec_tray::stop(app);
      }
    });
}
