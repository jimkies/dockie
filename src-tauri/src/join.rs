use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum JoinTarget {
    Home,
    Game { place_id: u64 },
    PrivateServer { place_id: u64, link_code: String },
    ShareLink { code: String },
    Server { place_id: u64, job_id: String },
    FollowUserId { user_id: u64 },
    FollowUsername { username: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Home,
    Game { place_id: u64 },
    PrivateServer { place_id: u64, link_code: String },
    Server { place_id: u64, job_id: String },
    Follow { user_id: u64 },
}

pub const PLACE_LAUNCHER: &str = "https://assetgame.roblox.com/game/PlaceLauncher.ashx";

const BAD: &str = "Couldn't read this link";

pub fn parse_join(input: &str) -> Result<JoinTarget, String> {
    use JoinTarget::*;
    let s = input.trim();
    if s.is_empty() {
        return Ok(Home);
    }
    if let Some((x, y)) = s.split_once(':') {
        if let Ok(place_id) = x.parse() {
            if !y.is_empty() && y.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Ok(Server { place_id, job_id: y.into() });
            }
        }
    }
    let url = match s.strip_prefix("roblox://") {
        // Bare `roblox://placeId=..` is all query; `roblox://experiences/start?..` has a path.
        Some(rest) if rest.contains(['/', '?']) => Url::parse(&format!("https://www.roblox.com/{rest}")),
        Some(rest) => Url::parse(&format!("https://www.roblox.com/?{rest}")),
        None if s.contains("://") => Url::parse(s),
        None => Url::parse(&format!("https://{s}")),
    };
    // Non-Roblox hosts fall through to the plain-text rules below.
    if let Some(u) = url.ok().filter(|u| u.host_str().is_some_and(|h| h == "roblox.com" || h.ends_with(".roblox.com"))) {
        let q = |k: &str| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned());
        let num = |v: Option<&str>| v.and_then(|v| v.parse::<u64>().ok());
        let query_place = num(q("placeId").as_deref());
        if let (Some(place_id), Some(job_id)) = (query_place, q("gameInstanceId").or_else(|| q("jobId"))) {
            return Ok(Server { place_id, job_id });
        }
        let segs: Vec<&str> = u.path_segments().map(|p| p.collect()).unwrap_or_default();
        match (segs.first().copied(), num(segs.get(1).copied())) {
            (Some("games"), Some(place_id)) => {
                return Ok(match q("privateServerLinkCode") {
                    Some(link_code) => PrivateServer { place_id, link_code },
                    None => Game { place_id },
                })
            }
            (Some("users"), Some(user_id)) => return Ok(FollowUserId { user_id }),
            (Some("share"), _) => {
                if let (Some(code), Some("Server")) = (q("code"), q("type").as_deref()) {
                    return Ok(ShareLink { code });
                }
            }
            _ => {}
        }
        if let Some(place_id) = query_place {
            return Ok(Game { place_id });
        }
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse().map(|place_id| Game { place_id }).map_err(|_| BAD.into());
    }
    let name = s.strip_prefix('@').unwrap_or(s);
    if (3..=20).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Ok(FollowUsername { username: name.into() });
    }
    Err(BAD.into())
}

pub fn launch_uri(ticket: &str, target: &Resolved, launch_time_ms: u64, tracker_id: u64) -> String {
    let t = tracker_id;
    let common = format!("browserTrackerId={t}&isPlayTogetherGame=false");
    let request = match target {
        Resolved::Home => String::new(),
        Resolved::Game { place_id } => format!("RequestGame&{common}&placeId={place_id}"),
        Resolved::PrivateServer { place_id, link_code } => format!("RequestPrivateGame&{common}&placeId={place_id}&linkCode={link_code}"),
        Resolved::Server { place_id, job_id } => format!("RequestGameJob&{common}&placeId={place_id}&gameId={job_id}"),
        Resolved::Follow { user_id } => format!("RequestFollowUser&{common}&userId={user_id}"),
    };
    let (mode, launcher) = if request.is_empty() {
        ("app", String::new())
    } else {
        let enc: String = url::form_urlencoded::byte_serialize(format!("{PLACE_LAUNCHER}?request={request}").as_bytes()).collect();
        ("play", format!("+placelauncherurl:{enc}"))
    };
    format!("roblox-player:1+launchmode:{mode}+gameinfo:{ticket}+launchtime:{launch_time_ms}{launcher}+browsertrackerid:{t}+robloxLocale:en_us+gameLocale:en_us")
}

#[cfg(test)]
mod tests {
    use super::*;
    use JoinTarget::*;

    const JOB: &str = "0f1e2d3c-aaaa-bbbb-cccc-111122223333";
    fn bad() -> Result<JoinTarget, String> {
        Err("Couldn't read this link".into())
    }

    #[test] fn empty() { assert_eq!(parse_join(""), Ok(Home)); }
    #[test] fn blank() { assert_eq!(parse_join("   \n"), Ok(Home)); }
    #[test] fn digits() { assert_eq!(parse_join("920587237"), Ok(Game { place_id: 920587237 })); }
    #[test] fn games_url() { assert_eq!(parse_join("https://www.roblox.com/games/920587237/Adopt-Me"), Ok(Game { place_id: 920587237 })); }
    #[test] fn schemeless_upper() { assert_eq!(parse_join("  WWW.ROBLOX.COM/games/920587237/Adopt-Me?foo=1 \n"), Ok(Game { place_id: 920587237 })); }
    #[test] fn web_host() { assert_eq!(parse_join("https://web.roblox.com/games/920587237"), Ok(Game { place_id: 920587237 })); }
    #[test] fn private() {
        assert_eq!(parse_join("https://www.roblox.com/games/920587237/Adopt-Me?privateServerLinkCode=12345678901234567890"),
            Ok(PrivateServer { place_id: 920587237, link_code: "12345678901234567890".into() }));
    }
    #[test] fn share() { assert_eq!(parse_join("https://www.roblox.com/share?code=abc123def&type=Server"), Ok(ShareLink { code: "abc123def".into() })); }
    #[test] fn start_instance() {
        assert_eq!(parse_join(&format!("https://www.roblox.com/games/start?placeId=920587237&gameInstanceId={JOB}")),
            Ok(Server { place_id: 920587237, job_id: JOB.into() }));
    }
    #[test] fn roblox_scheme() {
        assert_eq!(parse_join(&format!("roblox://placeId=920587237&jobId={JOB}")), Ok(Server { place_id: 920587237, job_id: JOB.into() }));
    }
    #[test] fn roblox_scheme_path() {
        assert_eq!(parse_join(&format!("roblox://experiences/start?placeId=920587237&gameInstanceId={JOB}")), Ok(Server { place_id: 920587237, job_id: JOB.into() }));
    }
    #[test] fn roblox_scheme_path_place_only() { assert_eq!(parse_join("roblox://experiences/start?placeId=920587237"), Ok(Game { place_id: 920587237 })); }
    #[test] fn x_colon_y() { assert_eq!(parse_join(&format!("920587237:{JOB}")), Ok(Server { place_id: 920587237, job_id: JOB.into() })); }
    #[test] fn start_place_only() { assert_eq!(parse_join("https://www.roblox.com/games/start?placeId=920587237"), Ok(Game { place_id: 920587237 })); }
    #[test] fn user_profile() { assert_eq!(parse_join("https://www.roblox.com/users/1234567/profile"), Ok(FollowUserId { user_id: 1234567 })); }
    #[test] fn username() { assert_eq!(parse_join("JakeAlt2"), Ok(FollowUsername { username: "JakeAlt2".into() })); }
    #[test] fn at_username() { assert_eq!(parse_join("@12345"), Ok(FollowUsername { username: "12345".into() })); }
    #[test] fn too_short() { assert_eq!(parse_join("ab"), bad()); }
    #[test] fn spaces() { assert_eq!(parse_join("this has spaces"), bad()); }
    #[test] fn foreign_host() { assert_eq!(parse_join("https://example.com/games/1"), bad()); }

    const TAIL: &str = "browsertrackerid:7+robloxLocale:en_us+gameLocale:en_us";
    #[test] fn uri_home() {
        assert_eq!(launch_uri("TICKET", &Resolved::Home, 1700000000000, 7),
            format!("roblox-player:1+launchmode:app+gameinfo:TICKET+launchtime:1700000000000+{TAIL}"));
    }
    #[test] fn uri_game() {
        assert_eq!(launch_uri("TICKET", &Resolved::Game { place_id: 920587237 }, 1700000000000, 7),
            format!("roblox-player:1+launchmode:play+gameinfo:TICKET+launchtime:1700000000000+placelauncherurl:https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestGame%26browserTrackerId%3D7%26isPlayTogetherGame%3Dfalse%26placeId%3D920587237+{TAIL}"));
    }
    #[test] fn uri_private() { assert!(launch_uri("T", &Resolved::PrivateServer { place_id: 1, link_code: "C".into() }, 0, 7).contains("RequestPrivateGame%26browserTrackerId%3D7%26isPlayTogetherGame%3Dfalse%26placeId%3D1%26linkCode%3DC")); }
    #[test] fn uri_server() { assert!(launch_uri("T", &Resolved::Server { place_id: 1, job_id: "J".into() }, 0, 7).contains("RequestGameJob%26browserTrackerId%3D7%26isPlayTogetherGame%3Dfalse%26placeId%3D1%26gameId%3DJ")); }
    #[test] fn uri_follow() { assert!(launch_uri("T", &Resolved::Follow { user_id: 5 }, 0, 7).contains("RequestFollowUser%26browserTrackerId%3D7%26isPlayTogetherGame%3Dfalse%26userId%3D5")); }
}
