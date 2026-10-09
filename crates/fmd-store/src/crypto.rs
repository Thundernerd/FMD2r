//! Encryption of account credentials at rest.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use crate::error::{Result, StoreError};

/// Encrypts and decrypts secrets stored in `app.db`. Injected into [`crate::AppDb::accounts`] so
/// the scheme can be swapped without touching the repositories.
pub trait Cipher: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;
}

/// File name of the [`KeyFileCipher`] key for account credentials, inside the data directory.
pub const ACCOUNTS_KEY_FILE: &str = "accounts.key";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

/// XChaCha20-Poly1305 with a random 256-bit key kept in a file (e.g. in the data directory).
/// Each ciphertext is `nonce || sealed`, with a fresh random nonce per call.
pub struct KeyFileCipher {
    aead: XChaCha20Poly1305,
}

impl KeyFileCipher {
    /// Loads the key from `path`, or generates one and writes it there (mode 0600 on Unix) when
    /// the file does not exist yet.
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let key = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => create_key_file(path)?,
            Err(e) => return Err(e.into()),
        };
        if key.len() != KEY_LEN {
            return Err(StoreError::Crypto(format!(
                "key file {} holds {} bytes, expected {KEY_LEN}",
                path.display(),
                key.len()
            )));
        }
        Ok(Self {
            aead: XChaCha20Poly1305::new(Key::from_slice(&key)),
        })
    }

    /// A cipher with a fresh random key that is never written anywhere, for an in-memory
    /// database whose secrets die with it.
    pub(crate) fn random() -> Self {
        Self {
            aead: XChaCha20Poly1305::new(&XChaCha20Poly1305::generate_key(&mut OsRng)),
        }
    }
}

/// Writes a fresh key to a temporary file next to `path` and publishes it with `hard_link`, which
/// fails if `path` already exists. Readers therefore never see a partially written key, and a
/// crash mid-write leaves only the temporary file behind.
fn create_key_file(path: &Path) -> Result<Vec<u8>> {
    let key = XChaCha20Poly1305::generate_key(&mut OsRng).to_vec();
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp_name);

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&tmp).and_then(|mut file| {
        file.write_all(&key)?;
        file.sync_all()
    });
    let linked = written.and_then(|()| fs::hard_link(&tmp, path));
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(key),
        // Another process created it first: use theirs.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(fs::read(path)?),
        Err(e) => Err(e.into()),
    }
}

impl Cipher for KeyFileCipher {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = self
            .aead
            .encrypt(&nonce, plaintext)
            .map_err(|_| StoreError::Crypto("encryption failed".into()))?;
        Ok([nonce.as_slice(), &sealed].concat())
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < NONCE_LEN {
            return Err(StoreError::Crypto("ciphertext too short".into()));
        }
        let (nonce, sealed) = ciphertext.split_at(NONCE_LEN);
        self.aead
            .decrypt(XNonce::from_slice(nonce), sealed)
            .map_err(|_| StoreError::Crypto("decryption failed (wrong key or corrupt data)".into()))
    }
}
