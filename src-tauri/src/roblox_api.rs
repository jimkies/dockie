use crate::join::{JoinTarget, Resolved};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Debug)]
pub enum ApiError {
  NeedsSignIn,
  Other(String),
}

impl std::fmt::Display for ApiError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ApiError::NeedsSignIn => write!(f, "Needs sign-in"),
      ApiError::Other(m) => write!(f, "{m}"),
    }
  }
}

impl std::error::Error for ApiError {}

// reqwest errors never include request headers, so no cookie can leak through here.
impl From<reqwest::Error> for ApiError {
  fn from(e: reqwest::Error) -> Self {
    ApiError::Other(e.without_url().to_string())
  }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
  pub id: u64,
  pub name: String,
  #[serde(rename = "displayName")]
  pub display_name: String,
}

/// Accepts a pasted cookie in any common shape and returns the bare value.
pub fn normalize_cookie(raw: &str) -> String {
  let s = raw.trim().trim_matches('"').trim();
  let s = s.strip_prefix(".ROBLOSECURITY=").unwrap_or(s);
  s.split(';').next().unwrap_or("").trim().trim_matches('"').to_string()
}

pub fn parse_share_resolve(v: &Value) -> Option<(u64, String)> {
  let d = v.get("privateServerInviteData")?;
  Some((d.get("placeId")?.as_u64()?, d.get("linkCode")?.as_str()?.to_string()))
}

#[derive(Debug, Serialize)]
pub struct GameHit {
  pub place_id: u64,
  pub universe_id: u64,
  pub name: String,
  pub players: u64,
  /// What a pick puts in the Join box; parse_join reads it as the game.
  pub url: String,
  pub icon: Option<String>,
}

/// Gives up on a call that hangs (Wi-Fi without internet, a dropped connection) instead of waiting forever.
async fn within<T>(limit: std::time::Duration, f: impl Future<Output = Result<T, ApiError>>) -> Result<T, ApiError> {
  tokio::time::timeout(limit, f).await.unwrap_or_else(|_| Err(ApiError::Other("Roblox took too long to answer".into())))
}

/// Games from an omni-search reply, at most eight, skipping entries Dockie can't join.
pub fn parse_game_search(v: &Value) -> Vec<GameHit> {
  let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
  list(&v["searchResults"])
    .iter()
    .filter(|g| g["contentGroupType"] == "Game")
    .flat_map(|g| list(&g["contents"]))
    .filter_map(|c| {
      let place_id = c["rootPlaceId"].as_u64()?;
      Some(GameHit {
        place_id,
        universe_id: c["universeId"].as_u64()?,
        name: c["name"].as_str()?.to_string(),
        players: c["playerCount"].as_u64().unwrap_or(0),
        url: match c["canonicalUrlPath"].as_str() {
          Some(p) if p.starts_with("/games/") => format!("roblox.com{p}"),
          _ => format!("roblox.com/games/{place_id}"),
        },
        icon: None,
      })
    })
    .take(8)
    .collect()
}

pub struct Api {
  client: Client,
}

impl Default for Api {
  fn default() -> Self {
    Self::new()
  }
}

impl Api {
  pub fn new() -> Api {
    // Redirects off so a cookie can never follow a redirect to another host.
    let client = Client::builder()
      .user_agent("Dockie/0.1")
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .expect("http client");
    Api { client }
  }

  fn req(&self, m: Method, url: &str, cookie: Option<&str>) -> RequestBuilder {
    let mut r = self.client.request(m, url);
    if let Some(c) = cookie {
      r = r.header("Cookie", format!(".ROBLOSECURITY={c}"));
    }
    if url.starts_with("https://auth.roblox.com/") {
      r = r.header("Referer", "https://www.roblox.com/").header("Origin", "https://www.roblox.com");
    }
    r
  }

  /// POST that answers a 403 + x-csrf-token by sending the request once more with the token.
  async fn post(&self, url: &str, cookie: &str, body: Option<&Value>) -> Result<Response, ApiError> {
    let send = |token: Option<String>| {
      let mut r = self.req(Method::POST, url, Some(cookie));
      if let Some(t) = token {
        r = r.header("x-csrf-token", t);
      }
      match body {
        Some(b) => r.json(b),
        None => r.header("Content-Length", "0"),
      }
      .send()
    };
    let resp = send(None).await?;
    let token = resp.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(String::from);
    let resp = match (resp.status(), token) {
      (StatusCode::FORBIDDEN, Some(t)) => send(Some(t)).await?,
      _ => resp,
    };
    if resp.status() == StatusCode::UNAUTHORIZED {
      return Err(ApiError::NeedsSignIn);
    }
    Ok(resp)
  }

  async fn get_json(&self, url: &str, cookie: Option<&str>) -> Result<Value, ApiError> {
    let resp = self.req(Method::GET, url, cookie).send().await?;
    if cookie.is_some() && resp.status() == StatusCode::UNAUTHORIZED {
      return Err(ApiError::NeedsSignIn);
    }
    Ok(resp.error_for_status()?.json().await?)
  }

  pub async fn authenticated_user(&self, cookie: &str) -> Result<UserInfo, ApiError> {
    let v = self.get_json("https://users.roblox.com/v1/users/authenticated", Some(cookie)).await?;
    serde_json::from_value(v).map_err(|e| ApiError::Other(e.to_string()))
  }

  pub async fn auth_ticket(&self, cookie: &str) -> Result<String, ApiError> {
    // Roblox answers 415 without a JSON body.
    let resp = self.post("https://auth.roblox.com/v1/authentication-ticket/", cookie, Some(&json!({}))).await?;
    resp
      .headers()
      .get("rbx-authentication-ticket")
      .and_then(|v| v.to_str().ok())
      .map(String::from)
      .ok_or_else(|| ApiError::Other(format!("Roblox gave no launch ticket (status {})", resp.status())))
  }

  pub async fn avatar_url(&self, user_id: u64) -> Result<Option<String>, ApiError> {
    let url = format!("https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={user_id}&size=150x150&format=Png");
    let v = self.get_json(&url, None).await?;
    Ok(v["data"][0]["imageUrl"].as_str().map(String::from))
  }

  pub async fn user_id_for_username(&self, name: &str) -> Result<Option<UserInfo>, ApiError> {
    let body = json!({"usernames": [name], "excludeBannedUsers": true});
    let resp = self.client.post("https://users.roblox.com/v1/usernames/users").json(&body).send().await?;
    let v: Value = resp.error_for_status()?.json().await?;
    Ok(v["data"].get(0).and_then(|u| serde_json::from_value(u.clone()).ok()))
  }

  pub async fn game_name(&self, place_id: u64) -> Result<Option<String>, ApiError> {
    let u = self.get_json(&format!("https://apis.roblox.com/universes/v1/places/{place_id}/universe"), None).await?;
    let Some(universe) = u["universeId"].as_u64() else { return Ok(None) };
    let g = self.get_json(&format!("https://games.roblox.com/v1/games?universeIds={universe}"), None).await?;
    Ok(g["data"][0]["name"].as_str().map(String::from))
  }

  async fn resolve_share_link(&self, cookie: &str, code: &str) -> Result<Resolved, ApiError> {
    let body = json!({"linkId": code, "linkType": "Server"});
    let resp = self.post("https://apis.roblox.com/sharelinks/v1/resolve-link", cookie, Some(&body)).await?;
    let v: Value = resp.error_for_status()?.json().await?;
    let (place_id, link_code) =
      parse_share_resolve(&v).ok_or_else(|| ApiError::Other("That private server link is no longer valid".into()))?;
    Ok(Resolved::PrivateServer { place_id, link_code })
  }

  pub async fn resolve(&self, cookie: &str, t: &JoinTarget) -> Result<Resolved, ApiError> {
    Ok(match t {
      JoinTarget::Home => Resolved::Home,
      JoinTarget::Game { place_id } => Resolved::Game { place_id: *place_id },
      JoinTarget::PrivateServer { place_id, link_code } => {
        Resolved::PrivateServer { place_id: *place_id, link_code: link_code.clone() }
      }
      JoinTarget::ShareLink { code } => return self.resolve_share_link(cookie, code).await,
      JoinTarget::Server { place_id, job_id } => Resolved::Server { place_id: *place_id, job_id: job_id.clone() },
      JoinTarget::FollowUserId { user_id } => Resolved::Follow { user_id: *user_id },
      JoinTarget::FollowUsername { username } => match self.user_id_for_username(username).await? {
        Some(u) => Resolved::Follow { user_id: u.id },
        None => return Err(ApiError::Other(format!("No Roblox user named {username}"))),
      },
    })
  }

  pub async fn search_games(&self, query: &str) -> Result<Vec<GameHit>, ApiError> {
    // omni-search returns nothing without a UUID-shaped sessionId; any value works, so make one per run.
    static SESSION: OnceLock<String> = OnceLock::new();
    let session = SESSION.get_or_init(|| {
      let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
      let h = format!("{n:032x}");
      format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    });
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let url = format!("https://apis.roblox.com/search-api/omni-search?searchQuery={q}&sessionId={session}&pageType=all");
    let mut hits = parse_game_search(&within(Duration::from_secs(8), self.get_json(&url, None)).await?);
    if hits.is_empty() {
      return Ok(hits);
    }
    let ids: Vec<String> = hits.iter().map(|h| h.universe_id.to_string()).collect();
    let url = format!(
      "https://thumbnails.roblox.com/v1/games/icons?universeIds={}&size=150x150&format=Png&isCircular=false",
      ids.join(",")
    );
    // Icons are decoration: if this call fails, show the games without them.
    if let Ok(icons) = within(Duration::from_secs(3), self.get_json(&url, None)).await {
      for i in icons["data"].as_array().into_iter().flatten() {
        if let (Some(id), Some(img)) = (i["targetId"].as_u64(), i["imageUrl"].as_str()) {
          for h in hits.iter_mut().filter(|h| h.universe_id == id) {
            h.icon = Some(img.to_string());
          }
        }
      }
    }
    Ok(hits)
  }

  async fn place_label(&self, place_id: u64) -> Result<String, ApiError> {
    Ok(self.game_name(place_id).await?.unwrap_or_else(|| format!("Place {place_id}")))
  }

  pub async fn describe(&self, _cookie: &str, t: &JoinTarget) -> Result<String, ApiError> {
    Ok(match t {
      JoinTarget::Home => "Home".into(),
      JoinTarget::Game { place_id } => self.place_label(*place_id).await?,
      JoinTarget::PrivateServer { place_id, .. } => format!("Private server in {}", self.place_label(*place_id).await?),
      JoinTarget::ShareLink { .. } => "Private server link".into(),
      JoinTarget::Server { place_id, .. } => format!("Server in {}", self.place_label(*place_id).await?),
      JoinTarget::FollowUserId { user_id } => format!("Follow user {user_id}"),
      JoinTarget::FollowUsername { username } => match self.user_id_for_username(username).await? {
        Some(u) => format!("Follow @{}", u.name),
        None => format!("Follow @{username}"),
      },
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalizes_cookies() {
    assert_eq!(normalize_cookie("  \"_|WARNING:x|_ABC\"\n"), "_|WARNING:x|_ABC");
    assert_eq!(normalize_cookie(".ROBLOSECURITY=_|WARNING:x|_ABC"), "_|WARNING:x|_ABC");
    assert_eq!(normalize_cookie("_|WARNING:x|_ABC; path=/"), "_|WARNING:x|_ABC");
  }

  #[test]
  fn parses_game_search() {
    let game = |id: u64, place: Option<u64>, path: Option<&str>| {
      let mut g = json!({"universeId": id, "name": format!("Game {id}"), "playerCount": 5});
      if let Some(p) = place {
        g["rootPlaceId"] = json!(p);
      }
      if let Some(p) = path {
        g["canonicalUrlPath"] = json!(p);
      }
      g
    };
    let v = json!({"searchResults": [
      {"contentGroupType": "Game", "contents": [
        game(994732206, Some(2753915549), Some("/games/2753915549/Blox-Fruits")),
        game(2, Some(20), None),
        game(3, None, None),
      ]},
      {"contentGroupType": "User", "contents": [game(4, Some(40), None)]},
    ]});
    let hits = parse_game_search(&v);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].place_id, 2753915549);
    assert_eq!(hits[0].universe_id, 994732206);
    assert_eq!(hits[0].name, "Game 994732206");
    assert_eq!(hits[0].players, 5);
    assert_eq!(hits[0].url, "roblox.com/games/2753915549/Blox-Fruits");
    assert_eq!(hits[0].icon, None);
    assert_eq!(hits[1].url, "roblox.com/games/20");
  }

  #[test]
  fn within_gives_up_on_a_hang() {
    let hang = std::future::pending::<Result<(), ApiError>>();
    let r = tauri::async_runtime::block_on(within(std::time::Duration::from_millis(20), hang));
    assert!(matches!(r, Err(ApiError::Other(m)) if m.contains("too long")));
  }

  #[test]
  fn game_search_keeps_eight() {
    let many: Vec<Value> =
      (1..=12u64).map(|i| json!({"universeId": i, "name": "g", "playerCount": 0, "rootPlaceId": i})).collect();
    let v = json!({"searchResults": [{"contentGroupType": "Game", "contents": many}]});
    assert_eq!(parse_game_search(&v).len(), 8);
  }

  #[test]
  fn parses_share_resolve() {
    let v = json!({"privateServerInviteData":{"placeId":920587237u64,"linkCode":"abc","status":"Valid"}});
    assert_eq!(parse_share_resolve(&v), Some((920587237, "abc".into())));
    assert_eq!(parse_share_resolve(&json!({})), None);
  }
}
