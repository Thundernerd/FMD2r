//! The server password (`server.auth_token`), stored as a salted Argon2id PHC hash. No FMD2
//! counterpart.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{ARGON2ID_IDENT, Argon2};
use fmd_store::Cipher;
use serde_json::Value;

use super::secrets::unseal;
use super::service::SettingsError;

const PASSWORD_KEY: &str = "auth_token";

fn hash(password: &str) -> Result<String, SettingsError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| SettingsError::Hash(e.to_string()))
}

/// Slow by design: call it off the async threads.
pub fn verify_password(hash: &str, candidate: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(candidate.as_bytes(), &hash)
            .is_ok()
    })
}

/// Whether `value` is a hash [`hash`] made, rather than a password.
fn is_hash(value: &str) -> bool {
    PasswordHash::new(value).is_ok_and(|h| h.algorithm == ARGON2ID_IDENT)
}

/// Hashes the password a merge patch sets; an empty one (clearing it) is left as is.
pub(super) fn hash_patched(patch: &mut Value) -> Result<(), SettingsError> {
    let slot = patch
        .get_mut("server")
        .and_then(|server| server.get_mut(PASSWORD_KEY));
    hash_in_place(slot, |_| true)
}

/// Replaces the non-empty password in `slot` with its hash when `needs_hash` says so.
fn hash_in_place(
    slot: Option<&mut Value>,
    needs_hash: impl FnOnce(&str) -> bool,
) -> Result<(), SettingsError> {
    if let Some(Value::String(password)) = slot
        && !password.is_empty()
        && needs_hash(password)
    {
        *password = hash(password)?;
    }
    Ok(())
}

/// Hashes a password an older build stored plain or encrypted; whether `group` changed.
///
/// One that cannot be decrypted (lost key file) becomes the hash of a random password, so the
/// API stays locked instead of turning open.
pub(super) fn hash_stored(cipher: &dyn Cipher, group: &mut Value) -> Result<bool, SettingsError> {
    let before = group.clone();
    let encrypted = group.get(PASSWORD_KEY).is_some_and(Value::is_object);
    unseal(cipher, group, &[PASSWORD_KEY]);
    if encrypted
        && group.get(PASSWORD_KEY).is_none()
        && let Value::Object(map) = group
    {
        tracing::error!(target: "fmd_core",
            "the server password cannot be decrypted; the API stays locked until a new one is \
             set (log in with --password / FMD2R_PASSWORD to set it)");
        let unknown = SaltString::generate(&mut OsRng);
        map.insert(PASSWORD_KEY.into(), hash(unknown.as_str())?.into());
    }
    hash_in_place(group.get_mut(PASSWORD_KEY), |p| !is_hash(p))?;
    Ok(*group != before)
}
