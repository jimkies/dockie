use crate::app::{AddResult, add_account};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const SITE: &str = "https://www.roblox.com";

/// Opens a Roblox login window and waits for the session cookie.
pub async fn sign_in(app: AppHandle, refresh: Option<u64>) -> Result<AddResult, String> {
  if let Some(w) = app.get_webview_window("login") {
    let _ = w.set_focus();
    return Err("Sign-in already open".into());
  }
  let url: url::Url = SITE.parse().map_err(|e: url::ParseError| e.to_string())?;
  let login_url = url.join("/login").map_err(|e| e.to_string())?;
  WebviewWindowBuilder::new(&app, "login", WebviewUrl::External(login_url))
    .title("Sign in to Roblox \u{2014} Dockie")
    .incognito(true)
    .inner_size(480.0, 720.0)
    .build()
    .map_err(|e| e.to_string())?;
  loop {
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let Some(w) = app.get_webview_window("login") else { return Err("cancelled".into()) };
    let found = w.cookies_for_url(url.clone()).ok().and_then(|c| c.into_iter().find(|c| c.name() == ".ROBLOSECURITY").map(|c| c.value().to_string()));
    if let Some(cookie) = found {
      let r = add_account(&app, &cookie, refresh).await;
      let _ = w.close();
      return r;
    }
  }
}
