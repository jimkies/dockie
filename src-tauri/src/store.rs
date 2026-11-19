use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
  Ok,
  NeedsSignIn,
  Unchecked,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Account {
  pub user_id: u64,
  pub username: String,
  pub display_name: String,
  pub avatar_url: Option<String>,
  pub added_at: u64,
  pub note: String,
  pub join: String,
  pub fps_cap: Option<i32>,
  pub cookie_dpapi: String,
  pub status: AccountStatus,
}

#[derive(Serialize, Deserialize)]
pub struct AccountsFile {
  pub version: u32,
  pub accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Settings {
  pub fps_cap_default: Option<i32>,
  pub trim_minimized: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LaunchedProc {
  pub pid: u32,
  pub started_at: u64,
  pub user_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct State {
  pub user_fps_cap: Option<i32>,
  pub restore_pending: bool,
  pub launched: Vec<LaunchedProc>,
}

#[derive(Debug, PartialEq)]
pub enum Upsert {
  Added,
  Replaced,
}

pub struct Store {
  pub dir: PathBuf,
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
  if let Some(p) = path.parent() {
    std::fs::create_dir_all(p)?;
  }
  let mut tmp = path.as_os_str().to_owned();
  tmp.push(".tmp");
  {
    // flushed before the rename so a crash can't leave an empty file in place of the old one
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
  }
  std::fs::rename(&tmp, path)
}

pub fn upsert(accounts: &mut Vec<Account>, new: Account) -> Upsert {
  match accounts.iter_mut().find(|a| a.user_id == new.user_id) {
    Some(old) => {
      // keep what the user set; take the fresh session data
      *old = Account { added_at: old.added_at, note: old.note.clone(), join: old.join.clone(), fps_cap: old.fps_cap, ..new };
      Upsert::Replaced
    }
    None => {
      accounts.push(new);
      Upsert::Added
    }
  }
}

fn load_or_default<T: DeserializeOwned + Default>(path: PathBuf) -> T {
  std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

impl Store {
  pub fn new(dir: PathBuf) -> Store {
    Store { dir }
  }

  pub fn load_accounts(&self) -> anyhow::Result<Vec<Account>> {
    match std::fs::read(self.dir.join("accounts.json")) {
      Ok(b) => Ok(serde_json::from_slice::<AccountsFile>(&b)?.accounts),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
      Err(e) => Err(e.into()),
    }
  }

  pub fn save_accounts(&self, a: &[Account]) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(&AccountsFile { version: 1, accounts: a.to_vec() })?;
    Ok(write_atomic(&self.dir.join("accounts.json"), &json)?)
  }

  pub fn load_settings(&self) -> Settings {
    load_or_default(self.dir.join("settings.json"))
  }

  pub fn save_settings(&self, s: &Settings) -> anyhow::Result<()> {
    Ok(write_atomic(&self.dir.join("settings.json"), &serde_json::to_vec_pretty(s)?)?)
  }

  pub fn load_state(&self) -> State {
    load_or_default(self.dir.join("state.json"))
  }

  pub fn save_state(&self, s: &State) -> anyhow::Result<()> {
    Ok(write_atomic(&self.dir.join("state.json"), &serde_json::to_vec_pretty(s)?)?)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Tmp(PathBuf);
  impl Tmp {
    fn new(name: &str) -> Tmp {
      Tmp(std::env::temp_dir().join(format!("dockie-test-{}-{name}", std::process::id())))
    }
  }
  impl Drop for Tmp {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }

  fn acct(id: u64) -> Account {
    Account {
      user_id: id,
      username: format!("u{id}"),
      display_name: format!("D{id}"),
      avatar_url: Some("http://a".into()),
      added_at: 5,
      note: "n".into(),
      join: "j".into(),
      fps_cap: Some(30),
      cookie_dpapi: "old".into(),
      status: AccountStatus::NeedsSignIn,
    }
  }

  #[test]
  fn load_accounts_missing_dir_is_empty() {
    let t = Tmp::new("missing");
    assert!(Store::new(t.0.join("none")).load_accounts().unwrap().is_empty());
  }

  #[test]
  fn load_accounts_corrupt_errors_and_file_untouched() {
    let t = Tmp::new("corrupt");
    std::fs::create_dir_all(&t.0).unwrap();
    std::fs::write(t.0.join("accounts.json"), "{not json").unwrap();
    assert!(Store::new(t.0.clone()).load_accounts().is_err());
    assert_eq!(std::fs::read_to_string(t.0.join("accounts.json")).unwrap(), "{not json");
  }

  #[test]
  fn accounts_round_trip() {
    let t = Tmp::new("rt");
    let s = Store::new(t.0.clone());
    let a = vec![acct(1), Account { avatar_url: None, fps_cap: None, status: AccountStatus::Ok, ..acct(2) }];
    s.save_accounts(&a).unwrap();
    assert_eq!(s.load_accounts().unwrap(), a);
  }

  #[test]
  fn write_atomic_leaves_no_tmp() {
    let t = Tmp::new("atomic");
    Store::new(t.0.clone()).save_accounts(&[acct(1)]).unwrap();
    assert!(t.0.join("accounts.json").exists());
    assert!(!t.0.join("accounts.json.tmp").exists());
  }

  #[test]
  fn upsert_adds_new() {
    let mut v = vec![];
    assert_eq!(upsert(&mut v, acct(1)), Upsert::Added);
    assert_eq!(v.len(), 1);
  }

  #[test]
  fn upsert_replaces_session_keeps_user_fields() {
    let mut v = vec![acct(1)];
    let new = Account { cookie_dpapi: "new".into(), username: "B".into(), note: "".into(), join: "".into(), fps_cap: None, added_at: 99, ..acct(1) };
    assert_eq!(upsert(&mut v, new), Upsert::Replaced);
    assert_eq!(v.len(), 1);
    assert_eq!((v[0].cookie_dpapi.as_str(), v[0].username.as_str()), ("new", "B"));
    assert_eq!((v[0].note.as_str(), v[0].join.as_str(), v[0].fps_cap, v[0].added_at), ("n", "j", Some(30), 5));
  }
}
