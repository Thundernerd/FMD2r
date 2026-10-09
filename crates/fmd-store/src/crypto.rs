//! Encryption of account credentials at rest.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use crate::error::{Result, StoreError};

/// Encrypts secrets stored in `app.db`; injected so the scheme can be swapped.
pub trait Cipher: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;
}

/// File name of the account credentials key, inside the data directory.
pub const ACCOUNTS_KEY_FILE: &str = "accounts.key";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

/// XChaCha20-Poly1305 with a key kept in a file; each ciphertext is `nonce || sealed`.
pub struct KeyFileCipher {
    aead: XChaCha20Poly1305,
}

impl KeyFileCipher {
    /// Loads the key, or creates it (mode 0600 on Unix) when missing.
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

    /// An unsaved random key, for an in-memory database.
    pub(crate) fn random() -> Self {
        Self {
            aead: XChaCha20Poly1305::new(&XChaCha20Poly1305::generate_key(&mut OsRng)),
        }
    }
}

/// Writes the key to a temporary file and publishes it with `hard_link`, which fails if `path`
/// exists, so readers never see a partial key and concurrent creators don't clobber each other.
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
