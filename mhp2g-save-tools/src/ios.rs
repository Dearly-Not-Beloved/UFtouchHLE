/*
 * mhp2g_save: Monster Hunter Portable 2nd G save converter
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version. See COPYING.
 */
//! The iOS outer layer: the game passes the game-layer ciphertext to
//! CCCrypt for AES-128-CBC with PKCS#7 padding, giving a 0x16A110-byte
//! `savedata.dat`.

use crate::aes::{decrypt_block, encrypt_block};
use crate::game::BODY_LEN;
use crate::Error;

pub const FILE_LEN: usize = BODY_LEN + 16;
const KEY: &[u8; 16] = b"ls6([f$]=~I(nf28";
const IV: &[u8; 16] = b";lknc(;f_wnX98<i";

fn cbc_encrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let pad = 16 - data.len() % 16;
    let mut out = data.to_vec();
    out.resize(data.len() + pad, pad as u8);
    let mut prev = *iv;
    for block in out.as_chunks_mut::<16>().0 {
        for (b, p) in block.iter_mut().zip(prev) {
            *b ^= p;
        }
        encrypt_block(key, block);
        prev = *block;
    }
    out
}

fn cbc_decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, Error> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(Error::new("AES data length is not a multiple of 16"));
    }
    let mut out = data.to_vec();
    let mut prev = *iv;
    for block in out.as_chunks_mut::<16>().0 {
        let cipher = *block;
        decrypt_block(key, block);
        for (b, p) in block.iter_mut().zip(prev) {
            *b ^= p;
        }
        prev = cipher;
    }
    let pad = *out.last().unwrap() as usize;
    if !(1..=16).contains(&pad) || out[out.len() - pad..].iter().any(|&b| b as usize != pad) {
        return Err(Error::new("bad PKCS#7 padding"));
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

/// `savedata.dat` -> game-layer ciphertext.
pub fn decrypt(file: &[u8]) -> Result<Vec<u8>, Error> {
    if file.len() != FILE_LEN {
        return Err(Error::new(format!(
            "an iOS savedata.dat is 0x{:X} bytes, not 0x{:X}",
            FILE_LEN,
            file.len()
        )));
    }
    let inner = cbc_decrypt(file, KEY, IV)?;
    if inner.len() != BODY_LEN {
        return Err(Error::new("unexpected padding"));
    }
    Ok(inner)
}

/// Game-layer ciphertext -> `savedata.dat`.
pub fn encrypt(inner: &[u8]) -> Vec<u8> {
    cbc_encrypt(inner, KEY, IV)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sp800_38a_cbc_vector() {
        let key: [u8; 16] = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let iv: [u8; 16] = core::array::from_fn(|i| i as u8);
        let plain = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        let cipher = cbc_encrypt(&plain, &key, &iv);
        assert_eq!(
            cipher[..16],
            [
                0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b, 0x12, 0xe9,
                0x19, 0x7d
            ]
        );
        assert_eq!(cbc_decrypt(&cipher, &key, &iv).unwrap(), plain);
    }

    #[test]
    fn padding() {
        for n in [0, 1, 15, 16, 17, 100] {
            let data: Vec<u8> = (0..n as u8).collect();
            let cipher = cbc_encrypt(&data, KEY, IV);
            assert_eq!(cipher.len(), (n / 16 + 1) * 16);
            assert_eq!(cbc_decrypt(&cipher, KEY, IV).unwrap(), data);
        }
    }
}
