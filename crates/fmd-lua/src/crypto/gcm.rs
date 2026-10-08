//! AES-GCM as FMD2 writes it out by hand (baseunits/BaseCrypto.pas:803-1063): any IV length,
//! the 16-byte tag appended to the ciphertext.

use super::Error;
use super::dcp::{Block, Rijndael};

/// Multiplication in GF(2^128) with GCM's bit order (`GFMult128`,
/// baseunits/BaseCrypto.pas:829-858).
fn gf_mult(x: u128, y: u128) -> u128 {
    const R: u128 = 0xE1 << 120;
    let mut z = 0;
    let mut v = x;
    for i in (0..128).rev() {
        if (y >> i) & 1 == 1 {
            z ^= v;
        }
        v = if v & 1 == 1 { (v >> 1) ^ R } else { v >> 1 };
    }
    z
}

/// Absorbs `data` zero-padded to whole blocks.
fn absorb(h: u128, mut x: u128, data: &[u8]) -> u128 {
    for chunk in data.chunks(16) {
        let mut b = [0u8; 16];
        b[..chunk.len()].copy_from_slice(chunk);
        x = gf_mult(x ^ u128::from_be_bytes(b), h);
    }
    x
}

/// `GHASH(H, AAD, C)` with the bit-length block (baseunits/BaseCrypto.pas:860-926).
fn ghash(h: u128, aad: &[u8], c: &[u8]) -> u128 {
    let x = absorb(h, absorb(h, 0, aad), c);
    let lens = (u128::from(aad.len() as u64 * 8) << 64) | u128::from(c.len() as u64 * 8);
    gf_mult(x ^ lens, h)
}

/// The keyed cipher, H, and the pre-counter block J0: the IV plus 0^31 1 for a 12-byte IV,
/// else GHASH of it (baseunits/BaseCrypto.pas:946-958). An invalid key size escapes the
/// Pascal as an exception, here an error.
fn setup(key: &[u8], iv: &[u8]) -> Result<(Rijndael, u128, Block), Error> {
    let r = Rijndael::new(key, None).ok_or(Error::InvalidKeySize)?;
    let h = u128::from_be_bytes(r.encrypt_block(&[0; 16]));
    let j0 = if iv.len() == 12 {
        let mut j = [0u8; 16];
        j[..12].copy_from_slice(iv);
        j[15] = 1;
        j
    } else {
        ghash(h, &[], iv).to_be_bytes()
    };
    Ok((r, h, j0))
}

/// `inc32` of the counter block's low 32 bits (`Inc32Block`, baseunits/BaseCrypto.pas:814-827).
fn inc32(b: &mut Block) {
    let n = u32::from_be_bytes([b[12], b[13], b[14], b[15]]).wrapping_add(1);
    b[12..].copy_from_slice(&n.to_be_bytes());
}

/// GCTR: the CTR keystream from inc32(J0) (baseunits/BaseCrypto.pas:960-979, 1038-1057).
fn gctr(r: &Rijndael, j0: &Block, data: &[u8]) -> Vec<u8> {
    let mut cb = *j0;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        inc32(&mut cb);
        let ks = r.encrypt_block(&cb);
        out.extend(chunk.iter().zip(ks).map(|(a, b)| a ^ b));
    }
    out
}

/// The tag: GHASH(AAD, C) xor E(J0) (baseunits/BaseCrypto.pas:981-983, 1029-1031).
fn tag(r: &Rijndael, h: u128, j0: &Block, aad: &[u8], c: &[u8]) -> Block {
    (ghash(h, aad, c) ^ u128::from_be_bytes(r.encrypt_block(j0))).to_be_bytes()
}

/// `AESEncryptGCM(s, key, iv, aad)`: ciphertext || tag; an empty `s`, key or IV gives ''
/// (baseunits/BaseCrypto.pas:928-991).
pub fn encrypt(s: &[u8], key: &[u8], iv: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
    if s.is_empty() || key.is_empty() || iv.is_empty() {
        return Ok(Vec::new());
    }
    let (r, h, j0) = setup(key, iv)?;
    let mut out = gctr(&r, &j0, s);
    let t = tag(&r, h, &j0, aad, &out);
    out.extend_from_slice(&t);
    Ok(out)
}

/// `AESDecryptGCM(s, key, iv, aad)`: '' when `s` is shorter than a tag, the key or IV is
/// empty, or the tag does not match (baseunits/BaseCrypto.pas:993-1063).
pub fn decrypt(s: &[u8], key: &[u8], iv: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
    if s.len() < 16 || key.is_empty() || iv.is_empty() {
        return Ok(Vec::new());
    }
    let (c, expected) = s.split_at(s.len() - 16);
    let (r, h, j0) = setup(key, iv)?;
    let t = tag(&r, h, &j0, aad, c);
    if !super::ct_eq(&t, expected) {
        return Ok(Vec::new());
    }
    Ok(gctr(&r, &j0, c))
}
