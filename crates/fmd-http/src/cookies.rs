//! `THTTPCookieManager`: the cookie jar shared by all sessions of one module.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::strings::NameValueList;
use crate::{HttpError, httpdate};

/// One stored cookie (`THTTPCookie`, baseunits/httpcookiemanager.pas:15-37).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub same_site: String,
    /// Expiry in Unix seconds; `Some` makes the cookie persistent.
    pub expires: Option<i64>,
    pub host_only: bool,
    pub http_only: bool,
    pub secure: bool,
}

/// A thread-safe cookie jar with FMD2's (not RFC 6265's) matching rules
/// (baseunits/httpcookiemanager.pas). Serialise it with [`to_json`](Self::to_json).
#[derive(Debug, Default)]
pub struct CookieJar {
    cookies: Mutex<Vec<Cookie>>,
}

fn unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `(protocol, host, path)` of a URL as Synapse's `ParseURL` gives them: host lowercased
/// by the URL parser, path without the query, `/` when empty.
fn parse_url(url: &str) -> Option<(String, String, String)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    Some((
        parsed.scheme().to_string(),
        parsed.host_str().unwrap_or_default().to_string(),
        parsed.path().to_string(),
    ))
}

/// `SeparateLeft`/`SeparateRight` on `=`, trimmed; the whole string is the name and the
/// value when there is no `=` (baseunits/synapse/synautil.pas).
fn split_pair(s: &str) -> (&str, &str) {
    match s.split_once('=') {
        Some((n, v)) => (n.trim(), v.trim()),
        None => (s.trim(), s.trim()),
    }
}

impl CookieJar {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Cookie>> {
        // A poisoned jar is still a valid list of cookies.
        self.cookies.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// All stored cookies.
    pub fn cookies(&self) -> Vec<Cookie> {
        self.lock().clone()
    }

    /// `AddServerCookies(url, cookies, serverDate)` (baseunits/httpcookiemanager.pas:175-188):
    /// one `Set-Cookie` value per line; `Max-Age` counts from `server_date`.
    pub fn add_server_cookies(&self, url: &str, cookies: &str, server_date: SystemTime) {
        if cookies.is_empty() {
            return;
        }
        let mut jar = self.lock();
        for line in cookies.trim().split('\n') {
            if let Some(cookie) = parse_set_cookie(url, line, unix(server_date)) {
                add_cookie(&mut jar, cookie);
            }
        }
    }

    /// The `Set-Cookie` lines of a Synapse response header list, dated by its `Date`
    /// header (baseunits/httpcookiemanager.pas:190-216).
    pub(crate) fn add_response_cookies(&self, url: &str, headers: &NameValueList) {
        let cookies: Vec<&str> = (0..headers.lines().len())
            .filter(|&i| {
                headers.lines()[i]
                    .to_ascii_lowercase()
                    .starts_with("set-cookie")
            })
            .map(|i| headers.value_at(i).trim())
            .collect();
        if cookies.is_empty() {
            return;
        }
        let date = headers.value("Date").trim();
        let date = if date.is_empty() {
            unix(SystemTime::now())
        } else {
            httpdate::parse(date).unwrap_or(0)
        };
        let mut jar = self.lock();
        for line in cookies {
            if let Some(cookie) = parse_set_cookie(url, line, date) {
                add_cookie(&mut jar, cookie);
            }
        }
    }

    /// `SetCookies` (baseunits/httpcookiemanager.pas:218-289): drops expired cookies and
    /// copies those matching `url` into `target` (`Cookies.Values[name] := value`).
    /// Domains match exactly or as a dot-separated suffix; paths match as FMD2 does,
    /// which only accepts a prefix when the cookie path ends in `/`. `Secure` is ignored.
    pub(crate) fn set_cookies(&self, url: &str, target: &mut NameValueList) {
        let Some((protocol, host, path)) = parse_url(url) else {
            return;
        };
        let protocol = protocol.to_lowercase();
        let host = host.to_lowercase();
        let now = unix(SystemTime::now());
        let mut jar = self.lock();
        jar.retain(|c| c.expires.is_none_or(|e| e > now));
        for c in jar.iter() {
            let domain_match = if c.host_only {
                host.eq_ignore_ascii_case(&c.domain)
            } else {
                !host.is_empty()
                    && !c.domain.is_empty()
                    && (host.eq_ignore_ascii_case(&c.domain)
                        || (host.ends_with(&c.domain)
                            && host[..host.len() - c.domain.len()].ends_with('.')))
            };
            // `CharEquals(Path, Length(c.Path), '/')` tests the prefix's own last char.
            let path_match = path.eq_ignore_ascii_case(&c.path)
                || (path.starts_with(&c.path)
                    && (c.path.ends_with('/')
                        || path.as_bytes().get(c.path.len().wrapping_sub(1)) == Some(&b'/')));
            let http_ok = !c.http_only || protocol == "http" || protocol == "https";
            if domain_match && path_match && http_ok {
                target.set_value(&c.name, &c.value);
            }
        }
    }

    /// `GetServerCookies(domain, name)` (baseunits/httpcookiemanager.pas:301-327): the
    /// domain's cookies (all, or those named `name`) as `Set-Cookie` lines joined by CRLF.
    pub fn get_server_cookies(&self, domain: &str, name: &str) -> String {
        let jar = self.lock();
        let lines: Vec<String> = jar
            .iter()
            .filter(|c| {
                c.domain.eq_ignore_ascii_case(domain)
                    && (name.is_empty() || c.name.eq_ignore_ascii_case(name))
            })
            .map(|c| {
                let mut line = format!(
                    "{}={}; domain={}; path={}",
                    c.name, c.value, c.domain, c.path
                );
                if let Some(expires) = c.expires {
                    line += &format!("; expires={}", httpdate::format(expires));
                }
                if c.secure {
                    line += "; secure";
                }
                if c.http_only {
                    line += "; httponly";
                }
                if c.same_site != "none" {
                    line += &format!("; samesite={}", c.same_site);
                }
                line
            })
            .collect();
        lines.join("\r\n")
    }

    /// `RemoveCookies(domain, name)` (baseunits/httpcookiemanager.pas:329-345): removes
    /// the domain's cookies, all of them when `name` is empty.
    pub fn remove_cookies(&self, domain: &str, name: &str) {
        self.lock().retain(|c| {
            !(c.domain.eq_ignore_ascii_case(domain)
                && (name.is_empty() || c.name.eq_ignore_ascii_case(name)))
        });
    }

    /// `Clear` (baseunits/httpcookiemanager.pas:291-299).
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The jar as JSON, for persisting in module settings.
    pub fn to_json(&self) -> Result<String, HttpError> {
        Ok(serde_json::to_string(&*self.lock())?)
    }

    /// Replaces the jar's contents with cookies saved by [`to_json`](Self::to_json).
    pub fn load_json(&self, json: &str) -> Result<(), HttpError> {
        let cookies: Vec<Cookie> = serde_json::from_str(json)?;
        *self.lock() = cookies;
        Ok(())
    }
}

/// `AddCookie` (baseunits/httpcookiemanager.pas:82-104): replaces a cookie with the same
/// name, domain and path.
fn add_cookie(jar: &mut Vec<Cookie>, cookie: Cookie) {
    if let Some(i) = jar
        .iter()
        .position(|c| c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path)
    {
        jar.remove(i);
    }
    jar.push(cookie);
}

/// `InternalAddServerCookie` (baseunits/httpcookiemanager.pas:106-173): domain and path
/// default to the request URL's host and full path (not RFC 6265's default-path).
fn parse_set_cookie(url: &str, line: &str, server_date: i64) -> Option<Cookie> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (_, host, path) =
        parse_url(url).unwrap_or_else(|| (String::new(), String::new(), "/".into()));
    let mut parts = line.split(';');
    let (name, value) = split_pair(parts.next()?);
    let mut cookie = Cookie {
        name: name.to_string(),
        value: value.to_string(),
        domain: host,
        path,
        same_site: String::new(),
        expires: None,
        host_only: false,
        http_only: false,
        secure: false,
    };
    for attribute in parts {
        let (n, v) = split_pair(attribute);
        match n.to_lowercase().as_str() {
            "domain" => cookie.domain = v.to_lowercase().trim_start_matches('.').to_string(),
            "path" => cookie.path = v.to_string(),
            "expires" => cookie.expires = Some(httpdate::parse(v).unwrap_or(0)),
            "max-age" => cookie.expires = Some(server_date + v.parse::<i64>().unwrap_or(0)),
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            "samesite" => cookie.same_site = v.to_lowercase(),
            _ => {}
        }
    }
    if cookie.same_site.is_empty() {
        cookie.same_site = "none".into();
    }
    Some(cookie)
}
