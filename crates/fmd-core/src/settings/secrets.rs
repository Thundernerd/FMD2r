//! Settings secrets at rest, stored as `{"encrypted": "<hex>"}` with `app.db`'s cipher (the
//! accounts' threat model). FMD2 only obfuscates the global proxy credentials with
//! `EncryptString` (mangadownloader/forms/frmMain.pas:5878-5879).

use fmd_store::{Cipher, StoreError};
use serde_json::{Map, Value};

/// Paths of the secrets inside each settings group's JSON.
const SETTINGS_SECRETS: &[(&str, &[&str])] = &[
    ("connections", &["proxy", "password"]),
    ("module_updater", &["github_token"]),
];

pub(super) const MODULE_HTTP_SECRET: &[&str] = &["proxy", "password"];

const ENCRYPTED: &str = "encrypted";

fn group_secrets(key: &str) -> impl Iterator<Item = &'static [&'static str]> {
    SETTINGS_SECRETS
        .iter()
        .filter(move |(group, _)| *group == key)
        .map(|(_, path)| *path)
}

/// Encrypts the plain non-empty string at `path` in `json`.
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

pub(super) fn seal_group(
    cipher: &dyn Cipher,
    key: &str,
    json: &mut Value,
) -> Result<(), StoreError> {
    group_secrets(key).try_for_each(|path| seal(cipher, json, path))
}

/// [`Found::Plain`] when one of the group's secrets was stored in plain text.
pub(super) fn unseal_group(cipher: &dyn Cipher, key: &str, json: &mut Value) -> Found {
    group_secrets(key).fold(Found::NotPlain, |found, path| {
        match unseal(cipher, json, path) {
            Found::Plain => Found::Plain,
            Found::NotPlain => found,
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Found {
    /// No plain secret: none, an empty one, or an encrypted one (now decrypted).
    NotPlain,
    /// A plain non-empty string, stored by a build that did not encrypt secrets.
    Plain,
}

/// Decrypts the secret at `path` in place. One that cannot be decrypted (lost key file) is
/// removed, so it falls back to unset.
pub(super) fn unseal(cipher: &dyn Cipher, json: &mut Value, path: &[&str]) -> Found {
    let Some(slot) = slot(json, path) else {
        return Found::NotPlain;
    };
    let opened = match slot {
        Value::String(plain) if !plain.is_empty() => return Found::Plain,
        Value::Object(map) => map
            .get(ENCRYPTED)
            .and_then(Value::as_str)
            .ok_or_else(|| "not an encrypted value".to_owned())
            .and_then(|sealed| open(cipher, sealed)),
        _ => return Found::NotPlain,
    };
    match opened {
        Ok(plain) => *slot = Value::String(plain),
        Err(reason) => {
            tracing::error!(target: "fmd_core",
                "the stored {} cannot be decrypted ({reason}); it is unset", path.join("."));
            remove(json, path);
        }
    }
    Found::NotPlain
}

fn open(cipher: &dyn Cipher, sealed: &str) -> Result<String, String> {
    let bytes = unhex(sealed).ok_or_else(|| "not hex".to_owned())?;
    let plain = cipher.decrypt(&bytes).map_err(|e| e.to_string())?;
    String::from_utf8(plain).map_err(|_| "not UTF-8".to_owned())
}

fn slot<'a>(json: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    path.iter().try_fold(json, |node, key| node.get_mut(*key))
}

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
