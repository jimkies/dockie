//! Localhost WebSocket server that dockie_antiafk.lua reports to, so each account shows whether the
//! executor is attached to its Roblox window.
use crate::logfile::log;
use serde::Serialize;
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::protocol::WebSocketConfig;

const STALE_AFTER: Duration = Duration::from_secs(30);
const MAX_CLIENTS: usize = 32;
const MAX_MESSAGE: usize = 4096;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Attach {
  Attached,
  Stale,
  Disconnected,
}

#[derive(Clone, Copy)]
struct Conn {
  id: u64,
  last_seen: Instant,
  closed: bool,
}

fn state(c: &Conn, now: Instant) -> Attach {
  if c.closed {
    Attach::Disconnected
  } else if now.duration_since(c.last_seen) > STALE_AFTER {
    Attach::Stale
  } else {
    Attach::Attached
  }
}

type Users = Arc<Mutex<HashMap<u64, Conn>>>;

#[derive(Default)]
pub struct Server {
  users: Users,
  listening: Mutex<Option<(u16, Arc<AtomicBool>)>>,
  next_id: Arc<AtomicU64>,
  clients: Arc<AtomicUsize>,
}

impl Server {
  /// Every user that has said hello since Dockie started, by Roblox user id.
  pub fn statuses(&self) -> HashMap<u64, Attach> {
    let now = Instant::now();
    self.users.lock().unwrap().iter().map(|(u, c)| (*u, state(c, now))).collect()
  }

  /// Listens on 127.0.0.1:port, replacing the listener on another port. Already on it: nothing to do.
  pub fn listen(&self, port: u16) -> Result<(), String> {
    let mut cur = self.listening.lock().unwrap();
    if cur.as_ref().is_some_and(|(p, _)| *p == port) {
      return Ok(());
    }
    if let Some((_, stop)) = cur.take() {
      stop.store(true, Ordering::SeqCst);
    }
    let l = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
      log(format!("attach: can't listen on port {port}: {e}"));
      format!("Port {port} is in use, so attach status is off. Pick another port in Settings.")
    })?;
    // polled, so a port change can stop it
    l.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    *cur = Some((port, stop.clone()));
    let (users, next_id, clients) = (self.users.clone(), self.next_id.clone(), self.clients.clone());
    log(format!("attach: listening on 127.0.0.1:{port}"));
    std::thread::spawn(move || {
      while !stop.load(Ordering::SeqCst) {
        match l.accept() {
          Ok((s, _)) if clients.load(Ordering::SeqCst) < MAX_CLIENTS => {
            let (users, id, clients) = (users.clone(), next_id.fetch_add(1, Ordering::SeqCst), clients.clone());
            std::thread::spawn(move || client(s, users, id, clients));
          }
          Ok(_) => {} // too many: dropping the stream closes it
          Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
      }
    });
    Ok(())
  }
}

struct Count(Arc<AtomicUsize>);
impl Drop for Count {
  fn drop(&mut self) {
    self.0.fetch_sub(1, Ordering::SeqCst);
  }
}

/// (type, userId) of a message from the script; None for anything malformed.
fn parse(text: &str) -> Option<(String, u64)> {
  let v: serde_json::Value = serde_json::from_str(text).ok()?;
  let kind = v.get("type")?.as_str()?.to_owned();
  let id = v.get("userId")?;
  let id = id.as_u64().or_else(|| id.as_f64().filter(|f| *f >= 1.0 && f.fract() == 0.0).map(|f| f as u64))?;
  Some((kind, id))
}

/// Browsers always send Origin and executors don't, so this keeps web pages from faking a status.
fn no_browsers(req: &Request, resp: Response) -> Result<Response, ErrorResponse> {
  if req.headers().contains_key("origin") {
    let mut r = ErrorResponse::new(Some("browsers may not connect".into()));
    *r.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
    return Err(r);
  }
  Ok(resp)
}

fn client(stream: TcpStream, users: Users, id: u64, clients: Arc<AtomicUsize>) {
  clients.fetch_add(1, Ordering::SeqCst);
  let _count = Count(clients);
  // a closed Roblox resets the socket; the timeout only frees slots held by silent junk connections
  if stream.set_nonblocking(false).is_err() || stream.set_read_timeout(Some(Duration::from_secs(300))).is_err() {
    return;
  }
  let cfg = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
  let Ok(mut ws) = tungstenite::accept_hdr_with_config(stream, no_browsers, Some(cfg)) else { return };
  let mut user: Option<u64> = None;
  loop {
    match ws.read() {
      Ok(tungstenite::Message::Text(t)) => {
        let Some((kind, uid)) = parse(t.as_str()) else { continue };
        let mut map = users.lock().unwrap();
        match kind.as_str() {
          "hello" => {
            // a rejoin opens a new socket before the old one closes; the newest one wins
            map.insert(uid, Conn { id, last_seen: Instant::now(), closed: false });
            if user.replace(uid) != Some(uid) {
              log(format!("attach: user {uid} attached"));
            }
          }
          "heartbeat" if user == Some(uid) => {
            if let Some(c) = map.get_mut(&uid).filter(|c| c.id == id) {
              c.last_seen = Instant::now();
            }
          }
          _ => {}
        }
      }
      Ok(tungstenite::Message::Close(_)) | Err(_) => break,
      Ok(_) => {}
    }
  }
  if let Some(uid) = user
    && let Some(c) = users.lock().unwrap().get_mut(&uid).filter(|c| c.id == id)
  {
    c.closed = true;
    log(format!("attach: user {uid} disconnected"));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::{Read, Write};

  #[test]
  fn status_follows_heartbeats_and_close() {
    let t = Instant::now();
    let c = Conn { id: 1, last_seen: t, closed: false };
    assert_eq!(state(&c, t + Duration::from_secs(29)), Attach::Attached);
    assert_eq!(state(&c, t + Duration::from_secs(31)), Attach::Stale);
    assert_eq!(state(&Conn { closed: true, ..c }, t), Attach::Disconnected);
  }

  #[test]
  fn parse_ignores_malformed_messages() {
    assert_eq!(parse(r#"{"type":"hello","userId":123,"username":"a"}"#), Some(("hello".into(), 123)));
    assert_eq!(parse(r#"{"type":"heartbeat","userId":4.0e9}"#), Some(("heartbeat".into(), 4_000_000_000)));
    for bad in ["", "nope", "{}", r#"{"type":1,"userId":1}"#, r#"{"type":"hello","userId":-1}"#, r#"{"type":"hello","userId":1.5}"#, r#"{"type":"hello"}"#] {
      assert_eq!(parse(bad), None, "{bad}");
    }
  }

  fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
  }

  fn handshake(port: u16, origin: bool) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let o = if origin { "Origin: http://evil.example\r\n" } else { "" };
    write!(s, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{o}\r\n").unwrap();
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf).unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
  }

  #[test]
  fn rejects_browsers_accepts_scripts_and_reports_busy_ports() {
    let port = free_port();
    let srv = Server::default();
    srv.listen(port).unwrap();
    srv.listen(port).unwrap(); // same port: no-op
    assert!(handshake(port, true).starts_with("HTTP/1.1 403"));
    assert!(handshake(port, false).starts_with("HTTP/1.1 101"));
    let other = Server::default();
    assert!(other.listen(port).unwrap_err().contains("in use"));
  }

  fn wait_for(srv: &Server, uid: u64, want: Option<Attach>) {
    for _ in 0..100 {
      if srv.statuses().get(&uid).copied() == want {
        return;
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    panic!("user {uid} never reached {want:?}: {:?}", srv.statuses().get(&uid));
  }

  #[test]
  fn hello_attaches_and_close_disconnects_but_a_rejoin_survives_the_old_close() {
    use tungstenite::Message;
    let port = free_port();
    let srv = Server::default();
    srv.listen(port).unwrap();
    let url = format!("ws://127.0.0.1:{port}");
    let open = || tungstenite::client(url.as_str(), TcpStream::connect(("127.0.0.1", port)).unwrap()).unwrap().0;

    let mut a = open();
    a.send(Message::text("garbage")).unwrap();
    a.send(Message::text(r#"{"type":"hello","userId":7,"username":"x","placeId":1,"jobId":"j"}"#)).unwrap();
    wait_for(&srv, 7, Some(Attach::Attached));

    let mut b = open(); // rejoin: a new socket says hello before the old one closes
    b.send(Message::text(r#"{"type":"hello","userId":7}"#)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    a.close(None).unwrap();
    let _ = a.read();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(srv.statuses().get(&7), Some(&Attach::Attached));

    b.close(None).unwrap();
    let _ = b.read();
    wait_for(&srv, 7, Some(Attach::Disconnected));
  }
}
