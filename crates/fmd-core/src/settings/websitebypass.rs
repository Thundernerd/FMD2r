//! Writes `websitebypass_config.json`, which `cloudflare.lua` reads to find FlareSolverr
//! (lua/websitebypass/cloudflare.lua:271-325).

use std::path::Path;

use serde_json::{Map, Value};

/// FlareSolverr's default port.
const FLARESOLVERR_PORT: u16 = 8191;

/// Writes `<lua_dir>/websitebypass/websitebypass_config.json` for `flaresolverr_url`.
///
/// A URL sets `use_webdriver` (how `cloudflare.lua` reaches FlareSolverr) with its host and
/// port; empty writes upstream's defaults, off and `localhost:8191`
/// (lua/websitebypass/websitebypass_config.json). A non-`http(s)` URL is `InvalidInput`. Other
/// keys keep their values.
pub fn write_websitebypass_config(lua_dir: &Path, flaresolverr_url: &str) -> std::io::Result<()> {
    let file = lua_dir.join("websitebypass/websitebypass_config.json");
    let mut config = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Map<String, Value>>(&bytes).ok())
        .unwrap_or_default();
    let (enabled, host, port) = match flaresolverr_address(flaresolverr_url) {
        Some((host, port)) => (true, host, port),
        None if flaresolverr_url.trim().is_empty() => {
            (false, "localhost".to_owned(), FLARESOLVERR_PORT)
        }
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("FlareSolverr URL {flaresolverr_url:?} is not an http(s) URL"),
            ));
        }
    };
    config.insert("use_webdriver".into(), enabled.into());
    for flag in ["debug", "testing"] {
        config.entry(flag).or_insert(false.into());
    }
    config.insert("flaresolverr_ip".into(), host.into());
    config.insert("flaresolverr_port".into(), port.into());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(&Value::Object(config)).map_err(std::io::Error::other)?;
    // Written whole and renamed into place: `cloudflare.lua` may read it at any time.
    let temp = file.with_file_name(".websitebypass_config.json.fmd2r-write");
    let written = std::fs::write(&temp, json).and_then(|()| std::fs::rename(&temp, &file));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// The host and port of an `http(s)://host[:port][/...]` URL; `None` when it is blank or names
/// no host or a bad port.
pub(super) fn flaresolverr_address(url: &str) -> Option<(String, u16)> {
    let url = url.trim();
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
        _ => (authority, FLARESOLVERR_PORT),
    };
    (!host.is_empty()).then(|| (host.to_owned(), port))
}
