use crate::store::{Account, AccountStatus};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ExportFile {
  pub dockie_export: u32,
  pub exported_at: u64,
  pub accounts: Vec<ExportAccount>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExportAccount {
  pub user_id: u64,
  pub username: String,
  pub display_name: String,
  pub note: String,
  pub join: String,
  pub fps_cap: Option<i32>,
  pub cookie: String,
}

pub fn to_export(accounts: &[Account], selected: &[u64], now: u64, unprotect: &dyn Fn(&str) -> anyhow::Result<String>) -> anyhow::Result<ExportFile> {
  let accounts = accounts
    .iter()
    .filter(|a| selected.is_empty() || selected.contains(&a.user_id))
    .map(|a| {
      Ok(ExportAccount {
        user_id: a.user_id,
        username: a.username.clone(),
        display_name: a.display_name.clone(),
        note: a.note.clone(),
        join: a.join.clone(),
        fps_cap: a.fps_cap,
        cookie: unprotect(&a.cookie_dpapi)?,
      })
    })
    .collect::<anyhow::Result<_>>()?;
  Ok(ExportFile { dockie_export: 1, exported_at: now, accounts })
}

pub fn parse_import(json: &str) -> anyhow::Result<Vec<ExportAccount>> {
  let v: serde_json::Value = serde_json::from_str(json).map_err(|_| anyhow::anyhow!("Not a Dockie export file"))?;
  if v.get("dockie_export").and_then(|n| n.as_u64()) != Some(1) {
    anyhow::bail!("Not a Dockie export file");
  }
  Ok(serde_json::from_value::<ExportFile>(v)?.accounts)
}

pub fn import_into(accounts: &mut Vec<Account>, items: Vec<ExportAccount>, now: u64, protect: &dyn Fn(&str) -> anyhow::Result<String>) -> anyhow::Result<(usize, usize)> {
  let (mut added, mut replaced) = (0, 0);
  for it in items {
    let mut acc = Account {
      user_id: it.user_id,
      username: it.username,
      display_name: it.display_name,
      avatar_url: None,
      added_at: now,
      note: it.note,
      join: it.join,
      fps_cap: it.fps_cap,
      cookie_dpapi: protect(&it.cookie)?,
      status: AccountStatus::Unchecked,
    };
    match accounts.iter_mut().find(|a| a.user_id == acc.user_id) {
      Some(old) => {
        acc.added_at = old.added_at;
        *old = acc;
        replaced += 1;
      }
      None => {
        accounts.push(acc);
        added += 1;
      }
    }
  }
  Ok((added, replaced))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn acct(id: u64, note: &str) -> Account {
    Account {
      user_id: id,
      username: format!("u{id}"),
      display_name: format!("U{id}"),
      avatar_url: Some("http://a".into()),
      added_at: 100,
      note: note.into(),
      join: String::new(),
      fps_cap: Some(30),
      cookie_dpapi: format!("enc{id}"),
      status: AccountStatus::Ok,
    }
  }
  fn three() -> Vec<Account> {
    vec![acct(1, "a"), acct(2, "b"), acct(3, "c")]
  }
  fn id(s: &str) -> anyhow::Result<String> {
    Ok(s.to_string())
  }
  fn ex(id: u64, note: &str) -> ExportAccount {
    ExportAccount { user_id: id, username: "x".into(), display_name: "X".into(), note: note.into(), join: "j".into(), fps_cap: None, cookie: format!("c{id}") }
  }

  #[test]
  fn export_selected_only() {
    let f = to_export(&three(), &[2], 5, &|s| Ok(format!("plain-{s}"))).unwrap();
    assert_eq!(f.accounts.len(), 1);
    assert_eq!(f.accounts[0].user_id, 2);
    assert_eq!(f.accounts[0].cookie, "plain-enc2");
  }

  #[test]
  fn export_none_selected_means_all() {
    assert_eq!(to_export(&three(), &[], 5, &id).unwrap().accounts.len(), 3);
  }

  #[test]
  fn export_file_has_version_1() {
    assert_eq!(to_export(&three(), &[], 5, &id).unwrap().dockie_export, 1);
  }

  #[test]
  fn import_rejects_other_json() {
    for bad in ["{\"accounts\":[]}", "{\"dockie_export\":2,\"exported_at\":0,\"accounts\":[]}", "[]", "nope"] {
      assert!(parse_import(bad).is_err(), "{bad}");
    }
    assert_eq!(parse_import("{\"accounts\":[]}").unwrap_err().to_string(), "Not a Dockie export file");
  }

  #[test]
  fn import_adds_and_replaces() {
    let mut list = vec![acct(1, "old")];
    let r = import_into(&mut list, vec![ex(1, "new"), ex(2, "n2")], 999, &|s| Ok(format!("p-{s}"))).unwrap();
    assert_eq!(r, (1, 1));
    assert_eq!(list[0].note, "new");
    assert_eq!(list[0].added_at, 100);
    assert_eq!(list[0].cookie_dpapi, "p-c1");
    assert_eq!(list[1].added_at, 999);
    assert!(list.iter().all(|a| a.status == AccountStatus::Unchecked && a.avatar_url.is_none()));
  }

  #[test]
  fn round_trip() {
    let f = to_export(&three(), &[], 5, &id).unwrap();
    let back = parse_import(&serde_json::to_string(&f).unwrap()).unwrap();
    assert_eq!(back, f.accounts);
  }
}
