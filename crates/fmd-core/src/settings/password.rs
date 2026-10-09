//! The server password (`server.auth_token`) is stored as a salted Argon2id hash in PHC string
//! form, never as the password: a patch that sets it is hashed before it is merged, and a
//! password an older build stored (plain, or encrypted as the other secrets are) is hashed on
//! load. No FMD2 counterpart: FMD2 has no web server.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{ARGON2ID_IDENT, Argon2};
use fmd_store::Cipher;
use serde_json::Value;

use super::secrets::unseal;
use super::service::SettingsError;

/// The key of the password inside the `server` group.
const PASSWORD_KEY: &str = "auth_token";

/// A salted hash of `password`.
fn hash(password: &str) -> Result<String, SettingsError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| SettingsError::Hash(e.to_string()))
}

/// Whether `candidate` is the password `hash` was made from. Slow by design: call it off the
/// async threads.
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

/// Replaces the password a merge patch sets with its hash. An empty one is left as it is: it
/// clears the password.
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

/// Hashes the password in the stored `server` group when an older build stored it plain or
/// encrypted; whether `group` changed.
///
/// An encrypted one that cannot be decrypted (a lost or replaced key file) is replaced by the
/// hash of a random password nobody knows, so the API stays locked instead of turning open;
/// `--password` / `FMD2R_PASSWORD` gets in to set a new one.
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
