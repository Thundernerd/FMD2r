//! `modules.json` → per-module settings, option values, cookie jars and accounts.
//!
//! Format: `TWebsiteModules.SaveToFile` (baseunits/WebsiteModules.pas:640-696) writes an array
//! with one object per module: `ID`, `Settings` (`TWebsiteModuleSettings` streamed by
//! `TJSONStreamer`, baseunits/WebsiteModulesSettings.pas:62-91), `Options` (option values by
//! name: booleans, strings, integers, :660-666), `Account` (`TWebsiteModuleAccount`, with
//! `Username`/`Password`/`Cookies` passed through `EncryptString`, :668-675) and `Cookies` (the
//! persistent `THTTPCookie`s, :676-682). `LoadFromFile` reads it back (:545-638).

use std::path::Path;

use fmd_core::settings::{
    Destination, HttpOverrides, LimitOverrides, ModuleOverrides, ProxyOverride, ProxyOverrideType,
    SettingsService,
};
use fmd_http::Cookie;
use fmd_lua::crypto::decrypt_string;
use fmd_store::{Account, AccountStatus, AppDb, Cipher};
use serde_json::{Map, Value, json};

use crate::ImportOptions;
use crate::error::ImportError;
use crate::fmd2::{
    json_bool, json_int, json_text, parse_datetime_text, read_json, tdatetime_to_ms,
    wall_clock_to_utc,
};
use crate::paths::translate;
use crate::report::{ImportReport, SkipReason, Unmapped};

const SOURCE: &str = "modules.json";

/// The parsed `modules.json`, or `None` when there is none.
pub(crate) fn read(path: &Path) -> Result<Option<Vec<Map<String, Value>>>, ImportError> {
    let entries: Option<Vec<Value>> = read_json(path)?;
    Ok(entries.map(|entries| {
        entries
            .into_iter()
            .filter_map(|e| match e {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .collect()
    }))
}

/// The module ids `modules.json` lists (FMD2 writes every installed module).
pub(crate) fn module_ids(entries: &[Map<String, Value>]) -> impl Iterator<Item = String> + '_ {
    entries.iter().map(|e| string(get(e, "ID")))
}

/// A property by name, also matched case-insensitively in case a hand-edited file changed the
/// spelling.
fn get<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).or_else(|| {
        object
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    })
}

fn object<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    get(object, name).and_then(Value::as_object)
}

fn string(value: Option<&Value>) -> String {
    json_text(value)
}

fn boolean(value: Option<&Value>) -> bool {
    json_bool(value).unwrap_or(false)
}

fn limit(value: Option<&Value>) -> u32 {
    json_int(value)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// An enumerated property: `TJSONStreamer` writes its identifier, an integer stream its ordinal.
/// Returns the ordinal within `names`.
fn enumerated(value: Option<&Value>, names: &[&str]) -> Option<usize> {
    match value {
        Some(Value::String(s)) => names.iter().position(|n| n.eq_ignore_ascii_case(s)),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n < names.len()),
        _ => None,
    }
}

/// `TProxyType` (baseunits/WebsiteModulesSettings.pas:11).
fn proxy_type(value: Option<&Value>) -> ProxyOverrideType {
    const NAMES: [&str; 5] = ["ptDefault", "ptDirect", "ptHTTP", "ptSOCKS4", "ptSOCKS5"];
    match enumerated(value, &NAMES) {
        Some(1) => ProxyOverrideType::Direct,
        Some(2) => ProxyOverrideType::Http,
        Some(3) => ProxyOverrideType::Socks4,
        Some(4) => ProxyOverrideType::Socks5,
        _ => ProxyOverrideType::Default,
    }
}

/// `TAccountStatus` (baseunits/WebsiteModules.pas:78); `asChecking` becomes `asUnknown` on load
/// (:612-613).
fn account_status(value: Option<&Value>) -> AccountStatus {
    const NAMES: [&str; 4] = ["asUnknown", "asChecking", "asValid", "asInvalid"];
    match enumerated(value, &NAMES) {
        Some(2) => AccountStatus::Valid,
        Some(3) => AccountStatus::Invalid,
        _ => AccountStatus::Unknown,
    }
}

/// The module's overrides from `Settings` and `Options`, plus the `Settings` values FMD2r has no
/// place for.
fn overrides(
    module_id: &str,
    entry: &Map<String, Value>,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> ModuleOverrides {
    let empty = Map::new();
    let settings = object(entry, "Settings").unwrap_or(&empty);
    let http = object(settings, "HTTP").unwrap_or(&empty);
    let proxy = object(http, "Proxy").unwrap_or(&empty);

    let unmapped = [
        (
            "Settings.UpdateListNumberOfThread",
            get(settings, "UpdateListNumberOfThread"),
        ),
        (
            "Settings.UpdateListDirectoryPageNumber",
            get(settings, "UpdateListDirectoryPageNumber"),
        ),
    ];
    for (key, value) in unmapped {
        let value = string(value);
        if !value.is_empty() && value != "0" {
            report.unmapped.push(Unmapped {
                source: SOURCE.into(),
                key: format!("{module_id} {key}"),
                value,
            });
        }
    }

    let save_to = string(object(settings, "OverrideSettings").and_then(|o| get(o, "SaveToPath")));
    let save_to = if save_to.trim().is_empty() {
        String::new()
    } else {
        translate(&opts.path_maps, &save_to, report)
    };

    ModuleOverrides {
        enabled: boolean(get(settings, "Enabled")),
        save_to,
        limits: LimitOverrides {
            max_task_limit: limit(get(settings, "MaxTaskLimit")),
            max_thread_per_task_limit: limit(get(settings, "MaxThreadPerTaskLimit")),
            max_connection_limit: limit(get(settings, "MaxConnectionLimit")),
        },
        http: HttpOverrides {
            user_agent: string(get(http, "UserAgent")),
            cookies: string(get(http, "Cookies")),
            proxy: ProxyOverride {
                kind: proxy_type(get(proxy, "ProxyType")),
                host: string(get(proxy, "ProxyHost")),
                port: string(get(proxy, "ProxyPort")),
                username: string(get(proxy, "ProxyUsername")),
                password: string(get(proxy, "ProxyPassword")),
            },
        },
        options: object(entry, "Options").cloned().unwrap_or_default(),
    }
}

/// A `THTTPCookie` (baseunits/httpcookiemanager.pas:15-37) as an `fmd-http` cookie. `Expires` is
/// a `TDateTime` streamed as text (`jsoDateTimeAsString`, baseunits/WebsiteModules.pas:655).
fn cookie(c: &Map<String, Value>, opts: &ImportOptions) -> Cookie {
    let expires = match get(c, "Expires") {
        Some(Value::String(s)) => parse_datetime_text(s),
        Some(Value::Number(n)) => n.as_f64().and_then(tdatetime_to_ms),
        _ => None,
    };
    let expires = wall_clock_to_utc(expires, opts);
    Cookie {
        name: string(get(c, "Name")),
        value: string(get(c, "Value")),
        domain: string(get(c, "Domain")),
        path: string(get(c, "Path")),
        same_site: string(get(c, "SameSite")),
        expires: expires
            .filter(|_| boolean(get(c, "Persistent")))
            .map(|ms| ms.div_euclid(1000)),
        host_only: boolean(get(c, "HostOnly")),
        http_only: boolean(get(c, "HttpOnly")),
        secure: boolean(get(c, "Secure")),
    }
}

/// `DecryptString` of an account field, as `LoadFromFile` does (baseunits/WebsiteModules.pas:609-611).
fn decrypt(field: &str, value: Option<&Value>) -> Result<String, String> {
    let value = string(value);
    if value.is_empty() {
        return Ok(value);
    }
    String::from_utf8(decrypt_string(value.as_bytes()))
        .map_err(|_| format!("{field} does not decrypt to UTF-8 text"))
}

fn account(module_id: &str, a: &Map<String, Value>) -> Result<Account, String> {
    Ok(Account {
        module_id: module_id.to_string(),
        enabled: boolean(get(a, "Enabled")),
        username: decrypt("Username", get(a, "Username"))?,
        password: decrypt("Password", get(a, "Password"))?,
        cookies: decrypt("Cookies", get(a, "Cookies"))?,
        status: account_status(get(a, "Status")),
    })
}

/// Imports each module's settings, option values, cookies and account; returns the download
/// folders of the modules whose settings were imported, for [`add_destinations`].
pub(crate) fn import(
    entries: Option<&[Map<String, Value>]>,
    db: &AppDb,
    cipher: &dyn Cipher,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> Result<Vec<String>, ImportError> {
    let mut folders = Vec::new();
    let Some(entries) = entries else {
        return Ok(folders);
    };
    report.module_settings.found = true;
    report.accounts.found = true;
    let settings_repo = db.module_settings();
    let accounts = db.accounts(cipher);

    for entry in entries {
        let module_id = string(get(entry, "ID"));
        if module_id.is_empty() {
            report
                .module_settings
                .skip("(no ID)", SkipReason::Invalid("no module id".into()));
            continue;
        }

        let overrides = overrides(&module_id, entry, opts, report);
        let cookies: Vec<Cookie> = get(entry, "Cookies")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_object)
                    .map(|c| cookie(c, opts))
                    .collect()
            })
            .unwrap_or_default();
        // FMD2 writes every installed module; those with no settings, option values or cookies
        // need no row. Option values are always written, defaults included, so every module that
        // declares options gets one.
        if overrides != ModuleOverrides::default() || !cookies.is_empty() {
            if settings_repo.get(&module_id)?.is_some() {
                report
                    .module_settings
                    .skip(module_id.clone(), SkipReason::AlreadyExists);
            } else {
                if !opts.dry_run {
                    overrides.save(&settings_repo, &module_id)?;
                    if !cookies.is_empty() {
                        let jar =
                            serde_json::to_vec(&cookies).map_err(fmd_store::StoreError::from)?;
                        settings_repo.set_cookie_jar(&module_id, Some(&jar))?;
                    }
                }
                report.module_settings.imported += 1;
                if !overrides.save_to.is_empty() {
                    folders.push(overrides.save_to.clone());
                }
            }
        }

        let Some(a) = object(entry, "Account") else {
            continue;
        };
        let account = match account(&module_id, a) {
            Ok(account) => account,
            Err(why) => {
                report
                    .accounts
                    .skip(module_id.clone(), SkipReason::Invalid(why));
                continue;
            }
        };
        let blank = !account.enabled
            && account.username.is_empty()
            && account.password.is_empty()
            && account.cookies.is_empty();
        if blank {
            continue;
        }
        if accounts.get(&module_id)?.is_some() {
            report
                .accounts
                .skip(module_id.clone(), SkipReason::AlreadyExists);
            continue;
        }
        if !opts.dry_run {
            accounts.upsert(&account)?;
        }
        report.accounts.imported += 1;
    }
    Ok(folders)
}

/// Makes each website download folder in `folders` that no destination has yet a destination
/// (T74), named after its last path component, numbered when that name is taken. A path that
/// differs only by trailing separators is the same folder.
pub(crate) fn add_destinations(
    settings: &SettingsService,
    folders: &[String],
    opts: &ImportOptions,
) -> Result<(), ImportError> {
    let same =
        |a: &str, b: &str| a.trim_end_matches(['/', '\\']) == b.trim_end_matches(['/', '\\']);
    let mut destinations = settings.get().saveto.destinations.clone();
    let before = destinations.len();
    for folder in folders {
        if destinations.iter().any(|d| same(&d.path, folder)) {
            continue;
        }
        let base = folder
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(folder)
            .to_string();
        let taken = |name: &str| {
            destinations
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case(name))
        };
        let mut name = base.clone();
        let mut n = 2;
        while taken(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        destinations.push(Destination {
            name,
            path: folder.clone(),
            default: false,
        });
    }
    if destinations.len() != before && !opts.dry_run {
        settings.update(json!({ "saveto": { "destinations": destinations } }))?;
    }
    Ok(())
}
