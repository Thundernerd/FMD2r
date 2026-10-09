//! Secrets at rest: the passwords and tokens among the settings are stored encrypted with
//! `app.db`'s cipher ([`AppDb::cipher`](fmd_store::AppDb::cipher), the `accounts.key` that
//! accounts use too), as `{"encrypted": "<hex>"}` in place of the plain string. FMD2 keeps them
//! in plain `settings.json`.
//!
//! The threat model is the accounts' (see `crate::accounts`): copies of `app.db` without the
//! key file reveal nothing, anyone holding the key file and the database reveals everything.

use fmd_store::{Cipher, StoreError};
use serde_json::{Map, Value};

/// The secrets of each settings group, as paths inside the group's JSON.
const SETTINGS_SECRETS: &[(&str, &[&str])] = &[
    ("connections", &["proxy", "password"]),
    ("module_updater", &["github_token"]),
    ("server", &["auth_token"]),
];

/// The secret in a module's HTTP overrides, as a path inside their JSON.
pub(super) const MODULE_HTTP_SECRET: &[&str] = &["proxy", "password"];

/// The key of the object an encrypted secret is stored as.
const ENCRYPTED: &str = "encrypted";

/// The paths of the secrets in the settings group stored under `key`.
fn group_secrets(key: &str) -> impl Iterator<Item = &'static [&'static str]> {
    SETTINGS_SECRETS
        .iter()
        .filter(move |(group, _)| *group == key)
        .map(|(_, path)| *path)
}

/// Encrypts the plain non-empty string at `path` in `json`. An empty or missing secret is left
/// as it is: there is nothing to hide.
pub(super) fn seal(cipher: &dyn Cipher, json: &mut Value, path: &[&str]) -> Result<(), StoreError> {
    let Some(slot) = slot(json, path) else {
        return Ok(());
    };
    if let Value::String(plain) = slot
        && !plain.is_empty()
    {
        let sealed = hex(&cipher.encrypt(plain.as_bytes())?);
        *slot = Value::Object(Map::from_iter([(ENCRYPTED.into(), sealed.into())]));
    }
    Ok(())
}

/// Encrypts every secret of the settings group `json` stored under `key`.
pub(super) fn seal_group(
    cipher: &dyn Cipher,
    key: &str,
    json: &mut Value,
) -> Result<(), StoreError> {
    group_secrets(key).try_for_each(|path| seal(cipher, json, path))
}

/// Decrypts every secret of the settings group `json` stored under `key`; [`Found::Plain`] when
/// one of them was stored in plain text.
pub(super) fn unseal_group(cipher: &dyn Cipher, key: &str, json: &mut Value) -> Found {
    group_secrets(key).fold(Found::Sealed, |found, path| {
        match unseal(cipher, json, path) {
            Found::Plain => Found::Plain,
            Found::Sealed => found,
        }
    })
}

/// What [`unseal`] found at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Found {
    /// Nothing to decrypt: no secret, an empty one, or one decrypted.
    Sealed,
    /// A plain non-empty string, stored by a build that did not encrypt secrets.
    Plain,
}

/// Decrypts the secret at `path` in `json` back to its plain string. A secret that cannot be
/// decrypted (a lost or replaced key file) is removed, so it falls back to its default (unset),
/// and logged.
pub(super) fn unseal(cipher: &dyn Cipher, json: &mut Value, path: &[&str]) -> Found {
    let Some(slot) = slot(json, path) else {
        return Found::Sealed;
    };
    let opened = match slot {
        Value::String(plain) if !plain.is_empty() => return Found::Plain,
        Value::Object(map) => map
            .get(ENCRYPTED)
            .and_then(Value::as_str)
            .ok_or_else(|| "not an encrypted value".to_owned())
            .and_then(|sealed| open(cipher, sealed)),
        _ => return Found::Sealed,
    };
    match opened {
        Ok(plain) => *slot = Value::String(plain),
        Err(reason) => {
            tracing::error!(target: "fmd_core",
                "the stored {} cannot be decrypted ({reason}); it is unset", path.join("."));
            remove(json, path);
        }
    }
    Found::Sealed
}

fn open(cipher: &dyn Cipher, sealed: &str) -> Result<String, String> {
    let bytes = unhex(sealed).ok_or_else(|| "not hex".to_owned())?;
    let plain = cipher.decrypt(&bytes).map_err(|e| e.to_string())?;
    String::from_utf8(plain).map_err(|_| "not UTF-8".to_owned())
}

/// The value at `path` in `json`, when there is one.
fn slot<'a>(json: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    path.iter().try_fold(json, |node, key| node.get_mut(*key))
}

/// Removes the value at `path` from `json`.
fn remove(json: &mut Value, path: &[&str]) {
    if let Some((last, parents)) = path.split_last()
        && let Some(Value::Object(map)) = slot(json, parents)
    {
        map.remove(*last);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}
