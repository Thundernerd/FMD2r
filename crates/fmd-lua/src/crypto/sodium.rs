//! X25519 and libsodium's `crypto_secretstream_xchacha20poly1305` pull side
//! (baseunits/BaseCrypto.pas:1065-1534).

use chacha20::cipher::consts::U10;
use chacha20::cipher::generic_array::GenericArray;
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use poly1305::Poly1305;
use poly1305::universal_hash::KeyInit;

/// `X25519_PublicKey(priv)`: the public key for a 32-byte private key (clamped, as OpenSSL
/// does), else '' (baseunits/BaseCrypto.pas:1380-1398).
pub fn x25519_public_key(private: &[u8]) -> Vec<u8> {
    match <[u8; 32]>::try_from(private) {
        Ok(k) => x25519_dalek::x25519(k, x25519_dalek::X25519_BASEPOINT_BYTES).to_vec(),
        Err(_) => Vec::new(),
    }
}

/// `X25519_SharedSecret(priv, pub)`: '' unless both keys are 32 bytes, and '' for an
/// all-zero result, which OpenSSL's `EVP_PKEY_derive` refuses (baseunits/BaseCrypto.pas:1400-1434).
pub fn x25519_shared_secret(private: &[u8], public: &[u8]) -> Vec<u8> {
    let (Ok(k), Ok(u)) = (<[u8; 32]>::try_from(private), <[u8; 32]>::try_from(public)) else {
        return Vec::new();
    };
    let shared = x25519_dalek::x25519(k, u);
    if shared == [0; 32] {
        return Vec::new();
    }
    shared.to_vec()
}

/// Length of the opaque state string: `TSecretStreamState` = key, nonce and 20 pad bytes
/// (baseunits/BaseCrypto.pas:1138-1142).
const STATE_LEN: usize = 64;
/// `crypto_secretstream_xchacha20poly1305_TAG_REKEY`.
const TAG_REKEY: u8 = 0x02;

/// The pull state, serialised for Lua as `TSecretStreamState`.
struct State {
    k: [u8; 32],
    nonce: [u8; 12],
}

impl State {
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(STATE_LEN);
        out.extend_from_slice(&self.k);
        out.extend_from_slice(&self.nonce);
        out.resize(STATE_LEN, 0);
        out
    }

    fn from_bytes(b: &[u8]) -> Option<State> {
        if b.len() != STATE_LEN {
            return None;
        }
        let mut st = State {
            k: [0; 32],
            nonce: [0; 12],
        };
        st.k.copy_from_slice(&b[..32]);
        st.nonce.copy_from_slice(&b[32..44]);
        Some(st)
    }

    /// ChaCha20 (IETF) keystream xored into `buf`, starting at block `ic`.
    fn xor(&self, buf: &mut [u8], ic: u32) {
        let mut c = chacha20::ChaCha20::new(&self.k.into(), &self.nonce.into());
        c.seek(u64::from(ic) * 64);
        c.apply_keystream(buf);
    }

    /// `SecretStreamCounterReset` (baseunits/BaseCrypto.pas:1436-1440).
    fn reset_counter(&mut self) {
        self.nonce[..4].copy_from_slice(&1u32.to_le_bytes());
    }

    /// `SecretStreamRekey` (baseunits/BaseCrypto.pas:1442-1452).
    fn rekey(&mut self) {
        let mut buf = [0u8; 40];
        buf[..32].copy_from_slice(&self.k);
        buf[32..].copy_from_slice(&self.nonce[4..]);
        self.xor(&mut buf, 0);
        self.k.copy_from_slice(&buf[..32]);
        self.nonce[4..].copy_from_slice(&buf[32..]);
        self.reset_counter();
    }
}

/// `SecretStream_InitPull(header, key)`: the 64-byte state for a 24-byte header and 32-byte
/// key, else `None` (baseunits/BaseCrypto.pas:1454-1473).
pub fn init_pull(header: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    if header.len() != 24 || key.len() != 32 {
        return None;
    }
    let k = chacha20::hchacha::<U10>(
        GenericArray::from_slice(key),
        GenericArray::from_slice(&header[..16]),
    );
    let mut st = State {
        k: k.into(),
        nonce: [0; 12],
    };
    st.reset_counter();
    st.nonce[4..].copy_from_slice(&header[16..]);
    Some(st.to_bytes())
}

/// One pulled chunk: the next state, the message and its tag.
pub struct Pulled {
    pub state: Vec<u8>,
    pub msg: Vec<u8>,
    pub tag: u8,
}

/// `SecretStream_Pull(state, chunk)`: authenticates and decrypts one chunk (tag byte,
/// ciphertext, 16-byte MAC); `None` for a bad state, a chunk under 17 bytes or a MAC mismatch
/// (baseunits/BaseCrypto.pas:1475-1534).
///
/// The MAC covers the 64-byte tag block, the ciphertext, `mlen and 15` zero bytes (libsodium's
/// `(0x10 - sizeof block + mlen) & 0xf`, not a pad to a block boundary), a zero AD length and
/// `64 + mlen`, all little-endian (:1499-1511).
pub fn pull(state: &[u8], chunk: &[u8]) -> Option<Pulled> {
    let mut st = State::from_bytes(state)?;
    if chunk.len() < 17 {
        return None;
    }
    let (c, stored_mac) = chunk[1..].split_at(chunk.len() - 17);

    let mut block0 = [0u8; 64];
    st.xor(&mut block0, 0);
    let poly = Poly1305::new(GenericArray::from_slice(&block0[..32]));

    let mut block = [0u8; 64];
    st.xor(&mut block, 1);
    let tag = chunk[0] ^ block[0];
    block[0] = chunk[0];
    let mut authenticated = Vec::with_capacity(64 + c.len() + 15 + 16);
    authenticated.extend_from_slice(&block);
    authenticated.extend_from_slice(c);
    authenticated.resize(authenticated.len() + (c.len() & 15) + 8, 0);
    authenticated.extend_from_slice(&(64 + c.len() as u64).to_le_bytes());
    let mac = poly.compute_unpadded(&authenticated);

    if !super::ct_eq(&mac, stored_mac) {
        return None;
    }

    let mut msg = c.to_vec();
    st.xor(&mut msg, 2);

    for (n, m) in st.nonce[4..].iter_mut().zip(&mac[..8]) {
        *n ^= m;
    }
    let counter =
        u32::from_le_bytes([st.nonce[0], st.nonce[1], st.nonce[2], st.nonce[3]]).wrapping_add(1);
    st.nonce[..4].copy_from_slice(&counter.to_le_bytes());
    if tag & TAG_REKEY != 0 || counter == 0 {
        st.rekey();
    }
    Some(Pulled {
        state: st.to_bytes(),
        msg,
        tag,
    })
}
