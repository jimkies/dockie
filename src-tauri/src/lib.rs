pub mod app;
pub mod commands;
pub mod dpapi;
pub mod fps;
pub mod join;
pub mod login;
pub mod preview;
pub mod procs;
pub mod roblox_api;
pub mod store;
pub mod transfer;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
      let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
      app.manage(app::AppCtx::new(std::path::PathBuf::from(local).join("Dockie")));
      tauri::async_runtime::spawn(app::check_all(app.handle().clone()));
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      commands::get_snapshot,
      commands::add_cookie,
      commands::begin_login,
      commands::cancel_login,
      commands::remove_account,
      commands::set_note,
      commands::set_join,
      commands::set_fps_cap,
      commands::set_settings,
      commands::check_accounts,
      commands::export_accounts,
      commands::import_accounts,
      commands::open_data_folder,
    ])
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
