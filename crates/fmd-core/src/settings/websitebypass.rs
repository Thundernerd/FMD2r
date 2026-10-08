//! `lua/websitebypass/websitebypass_config.json`, which upstream's `cloudflare.lua` reads to
//! decide whether to ask FlareSolverr for Cloudflare cookies, and where it runs
//! (lua/websitebypass/cloudflare.lua:271-325). FMD2r writes it from the `flaresolverr_url`
//! setting at startup.

use std::path::Path;

use serde_json::{Map, Value};

/// FlareSolverr's own port, used when the URL names none.
const FLARESOLVERR_PORT: u16 = 8191;

/// Writes `<lua_dir>/websitebypass/websitebypass_config.json` for `flaresolverr_url`.
///
/// A URL turns the webdriver path on (`use_webdriver`, which is how `cloudflare.lua` reaches
/// FlareSolverr) with its host and port (8191 when it names none); an empty one writes
/// upstream's defaults: off, `localhost:8191` (lua/websitebypass/websitebypass_config.json). A
/// URL that is not `http(s)://host[:port]` is an `InvalidInput` error and writes nothing.
/// `debug`, `testing` and keys FMD2r does not know keep the values the file has.
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
    std::fs::write(file, json)
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
