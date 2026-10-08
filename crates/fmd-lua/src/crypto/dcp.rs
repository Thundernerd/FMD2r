//! The parts of DCPcrypt's `TDCP_rijndael` that FMD2 relies on: key setup, the nil-IV
//! default, and the CBC and 8-bit CFB modes with DCPcrypt's handling of a partial last block.
//! Paths cite the DCPcrypt 2.0.6 Lazarus sources FMD2 builds against.

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};

/// One AES block.
pub type Block = [u8; 16];

/// A keyed Rijndael with DCPcrypt's chaining value.
pub struct Rijndael {
    cipher: Cipher,
    cv: Block,
}

enum Cipher {
    Aes128(aes::Aes128),
    Aes192(aes::Aes192),
    Aes256(aes::Aes256),
}

impl Rijndael {
    /// `Init(Key, Length(Key) * 8, IV)`: the key is zero-padded to 16, 24 or 32 bytes
    /// (Ciphers/dcprijndael.pas:123-146); an empty key or one over 32 bytes is an invalid key
    /// size and `None` (dcpcrypt2.pas:417-425). A missing IV becomes E(0^16)
    /// (dcpblockciphers.pas:475-490); a given one is its first 16 bytes, zero-padded, as
    /// FMD2's callers build it (baseunits/BaseCrypto.pas:188-190).
    pub fn new(key: &[u8], iv: Option<&[u8]>) -> Option<Rijndael> {
        let mut k = [0u8; 32];
        k.get_mut(..key.len())?.copy_from_slice(key);
        let cipher = match key.len() {
            0 => return None,
            1..=16 => Cipher::Aes128(aes::Aes128::new(GenericArray::from_slice(&k[..16]))),
            17..=24 => Cipher::Aes192(aes::Aes192::new(GenericArray::from_slice(&k[..24]))),
            _ => Cipher::Aes256(aes::Aes256::new(GenericArray::from_slice(&k))),
        };
        let mut r = Rijndael {
            cipher,
            cv: [0; 16],
        };
        match iv {
            Some(iv) => r.set_iv(iv),
            None => r.cv = r.encrypt_block(&[0; 16]),
        }
        Some(r)
    }

    /// `SetIV`: the first 16 bytes, zero-padded (dcpblockciphers.pas:492-498).
    pub fn set_iv(&mut self, iv: &[u8]) {
        self.cv = [0; 16];
        let n = iv.len().min(16);
        self.cv[..n].copy_from_slice(&iv[..n]);
    }

    /// `EncryptECB` of one block (Ciphers/dcprijndael.pas:219).
    pub fn encrypt_block(&self, b: &Block) -> Block {
        let mut out = GenericArray::clone_from_slice(b);
        match &self.cipher {
            Cipher::Aes128(c) => c.encrypt_block(&mut out),
            Cipher::Aes192(c) => c.encrypt_block(&mut out),
            Cipher::Aes256(c) => c.encrypt_block(&mut out),
        }
        out.into()
    }

    /// `DecryptECB` of one block (Ciphers/dcprijndael.pas:285).
    pub fn decrypt_block(&self, b: &Block) -> Block {
        let mut out = GenericArray::clone_from_slice(b);
        match &self.cipher {
            Cipher::Aes128(c) => c.decrypt_block(&mut out),
            Cipher::Aes192(c) => c.decrypt_block(&mut out),
            Cipher::Aes256(c) => c.decrypt_block(&mut out),
        }
        out.into()
    }

    /// `EncryptCFB8bit` (dcpblockciphers.pas:577-597).
    pub fn encrypt_cfb8(&mut self, data: &[u8]) -> Vec<u8> {
        self.cfb8(data, true)
    }

    /// `DecryptCFB8bit` (dcpblockciphers.pas:599-621).
    pub fn decrypt_cfb8(&mut self, data: &[u8]) -> Vec<u8> {
        self.cfb8(data, false)
    }

    /// 8-bit CFB: each byte is xored with the first byte of E(CV), and the ciphertext byte is
    /// shifted into CV.
    fn cfb8(&mut self, data: &[u8], encrypt: bool) -> Vec<u8> {
        data.iter()
            .map(|&b| {
                let out = b ^ self.encrypt_block(&self.cv)[0];
                self.cv.copy_within(1.., 0);
                self.cv[15] = if encrypt { out } else { b };
                out
            })
            .collect()
    }

    /// Xors a trailing partial block with E(CV), as both CBC directions do
    /// (dcpblockciphers.pas:540-545, 569-574).
    fn cbc_tail(&mut self, tail: &[u8], out: &mut Vec<u8>) {
        if !tail.is_empty() {
            self.cv = self.encrypt_block(&self.cv);
            out.extend(tail.iter().zip(self.cv).map(|(a, b)| a ^ b));
        }
    }

    /// `EncryptCBC` (dcpblockciphers.pas:522-546).
    pub fn encrypt_cbc(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let blocks = data.chunks_exact(16);
        let tail = blocks.remainder();
        for block in blocks {
            let mut b = self.cv;
            b.iter_mut().zip(block).for_each(|(x, p)| *x ^= p);
            self.cv = self.encrypt_block(&b);
            out.extend_from_slice(&self.cv);
        }
        self.cbc_tail(tail, &mut out);
        out
    }

    /// `DecryptCBC` (dcpblockciphers.pas:548-575).
    pub fn decrypt_cbc(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let blocks = data.chunks_exact(16);
        let tail = blocks.remainder();
        for block in blocks {
            let mut c = [0u8; 16];
            c.copy_from_slice(block);
            let mut p = self.decrypt_block(&c);
            p.iter_mut().zip(self.cv).for_each(|(x, v)| *x ^= v);
            self.cv = c;
            out.extend_from_slice(&p);
        }
        self.cbc_tail(tail, &mut out);
        out
    }
}
