/*
 * mhp2g_save: Monster Hunter Portable 2nd G save converter
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version. See COPYING.
 */
//! Capcom's "game layer", which is the same on PSP and iOS apart from the
//! salt of the hash.
//!
//! The 0x16A100-byte save body:
//!
//! | offset              | contents                                        |
//! |---------------------|-------------------------------------------------|
//! | 0x000000..0x16A0DC  | save data (0x00..0x24: platform settings)       |
//! | 0x16A0DC..0x16A0E8  | metadata, not covered by the hash               |
//! | 0x16A0E8..0x16A0FC  | SHA-1(data[0x0..0x16A0DC] + salt)               |
//! | 0x16A0FC..0x16A100  | seed of the XOR stream                          |
//!
//! Each 32-bit little-endian word except the seed is encrypted as
//! `w = T[w bytes]; advance the LCG; w ^= beta; w = T[w bytes]`, where
//! `lo = lo * 0x215F % 0xFF8F`, `hi = hi * 0xDFA3 % 0xFFEF`,
//! `beta = lo | hi << 16`, starting from the seed (halves of 0 are replaced
//! by 0x215F and 0xDFA3). The seed is stored as `T[T[seed bytes]]`.
//!
//! On iOS this matches the game's code at 0x322704 (load), 0x322868 (game
//! layer) and 0x322AF4 (save) in the MHP2G binary.

use sha1::{Digest, Sha1};

pub const BODY_LEN: usize = 0x16A100;
pub const HASHED_LEN: usize = 0x16A0DC;
pub const META_OFF: usize = 0x16A0DC;
pub const HASH_OFF: usize = 0x16A0E8;
pub const SEED_OFF: usize = 0x16A0FC;
/// Platform-specific settings at the start of the save. The rest of the
/// first 0x1000 bytes is identical in all samples from both platforms.
pub const HEADER_LEN: usize = 0x24;
pub const NAME_OFF: usize = 0x1000;
pub const NAME_LEN: usize = 0x20;

/// The three character slots.
pub const SLOT_OFF: usize = 0x1000;
pub const SLOT_LEN: usize = 0x6B100;
pub const SLOTS: usize = 3;
/// The end of each slot (relative to the slot), from the character's own
/// guild card on, is where the platforms differ: the PSP has 0x28F8 bytes
/// more in it, inserted in a few places, and packs a few fields differently.
pub const IOS_TAIL: std::ops::Range<usize> = 0x649A0..SLOT_LEN;
/// The tail in parts: (iOS range, PSP offset - iOS offset). They hold the
/// character's own guild card (name, equipment, comment, records, quest
/// history), the money (iOS +0x66960), a list of 0x70-byte records, and the
/// tables, flags and counters at the end of the character data. Everything
/// but [MONSTER_RECORDS] has the same encoding on both platforms.
pub const SHIFTED: [(std::ops::Range<usize>, usize); 5] = [
    (0x649A0..0x64B78, 0x2884),
    (0x64B78..0x64B80, 0x2888),
    (0x64C78..0x65900, 0x2888),
    (0x65900..0x67420, 0x28F0),
    (0x67420..0x6803C, 0x28F8),
];
/// Guild card list of 31 monster records (iOS range, PSP offset - iOS
/// offset), 8 bytes each: the PSP has `u16 a, u16 b, u32 sizes`, iOS packs
/// the first two as `u32 a | b << 7` (`a` < 0x80; `b` is a hunt count).
pub const MONSTER_RECORDS: (std::ops::Range<usize>, usize) = (0x64B80..0x64C78, 0x2888);
/// Name field of the iOS guild card (PSP +0x672B8).
pub const IOS_GUILD_CARD_NAME: usize = 0x64A34;
/// Length of the name field at the start of each slot.
pub const SLOT_NAME_LEN: usize = 0x10;

const LO_MUL: u32 = 0x215F;
const LO_MOD: u32 = 0xFF8F;
const HI_MUL: u32 = 0xDFA3;
const HI_MOD: u32 = 0xFFEF;

/// Encryption table of both games (VA 0x5F67D0 in the iOS binary; the
/// decryption table follows at 0x5F68D0). This is not the table in mhef.
pub const ENC_TABLE: [u8; 256] = [
    0x3A, 0x11, 0x72, 0x6E, 0x2F, 0x7A, 0x23, 0xF0, 0x6B, 0x5C, 0xCA, 0x41, 0x57, 0x5D, 0xB2, 0xA8,
    0x06, 0x5F, 0xC9, 0xD8, 0x17, 0x82, 0xBB, 0xD2, 0x88, 0x86, 0x27, 0x47, 0xC7, 0x29, 0x5B, 0x04,
    0x8E, 0xAE, 0x5E, 0x33, 0x49, 0xDB, 0xA5, 0x7B, 0x31, 0xB7, 0xB5, 0x84, 0xF6, 0x76, 0x81, 0xAD,
    0x6F, 0xE0, 0x39, 0x87, 0xC0, 0xAB, 0x68, 0x4D, 0x05, 0x92, 0x77, 0xFF, 0x99, 0x36, 0x2C, 0x07,
    0x0A, 0x59, 0xFA, 0xBC, 0x54, 0xE1, 0x94, 0x3D, 0x1F, 0x58, 0xA6, 0xAA, 0x53, 0x2E, 0x89, 0xEB,
    0x9C, 0x98, 0x1B, 0xD6, 0xC1, 0x30, 0x00, 0xF4, 0xEF, 0x7D, 0xBF, 0xE8, 0x0C, 0xC4, 0xAC, 0xF3,
    0xEA, 0x78, 0xBE, 0xAF, 0xB0, 0x08, 0xA1, 0x90, 0x60, 0x2A, 0xD4, 0x62, 0x3B, 0xA7, 0xD9, 0xED,
    0x7C, 0xF5, 0xEC, 0xBA, 0x9B, 0x4A, 0x0F, 0x3C, 0x7F, 0xC6, 0x24, 0x55, 0x01, 0x43, 0x22, 0x8A,
    0xA0, 0x70, 0x65, 0x67, 0x50, 0x1D, 0x96, 0x42, 0xCE, 0x20, 0x03, 0x12, 0xE3, 0x4E, 0x95, 0x26,
    0xE4, 0xB8, 0xD7, 0x48, 0xD0, 0xC3, 0xB9, 0x80, 0x18, 0xD5, 0x02, 0x34, 0x21, 0x8B, 0xDF, 0xE9,
    0x61, 0x79, 0x0E, 0x93, 0xA3, 0x63, 0x1A, 0x9D, 0xB3, 0xCD, 0xE6, 0x14, 0x2D, 0xDC, 0xA4, 0x9E,
    0x28, 0x75, 0xA9, 0x66, 0x91, 0xBD, 0xD3, 0x6A, 0x6D, 0xFB, 0x5A, 0x8C, 0xE7, 0x64, 0x97, 0x19,
    0x46, 0xFE, 0x32, 0xF2, 0x9F, 0x38, 0x9A, 0x10, 0x3F, 0x6C, 0xE2, 0xD1, 0x45, 0x15, 0xB4, 0xDE,
    0x40, 0x56, 0xE5, 0x3E, 0xF9, 0xC8, 0x35, 0x16, 0x69, 0xFC, 0x85, 0x8F, 0x51, 0xA2, 0xC5, 0xEE,
    0xB6, 0x8D, 0x74, 0xCC, 0xB1, 0x4F, 0x25, 0xFD, 0x7E, 0x1C, 0x09, 0x13, 0x4B, 0x37, 0xCF, 0xDA,
    0x4C, 0xF7, 0xC2, 0x52, 0x2B, 0xF1, 0x1E, 0xF8, 0x0B, 0x83, 0xCB, 0x44, 0x0D, 0x73, 0x71, 0xDD,
];

const fn invert(table: &[u8; 256]) -> [u8; 256] {
    let mut inv = [0; 256];
    let mut i = 0;
    while i < 256 {
        inv[table[i] as usize] = i as u8;
        i += 1;
    }
    inv
}

pub const DEC_TABLE: [u8; 256] = invert(&ENC_TABLE);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Psp,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Psp => "psp",
        }
    }

    /// The salt of the save hash. The iOS one is the salt of the PSP's
    /// Monster Hunter Freedom Unite.
    pub fn salt(self) -> &'static [u8; 20] {
        match self {
            Platform::Ios => b"3Nc94Hq1zOLh8d62Sb69",
            Platform::Psp => b"S)R?Bf8xW3#5h9lGU8wR",
        }
    }

    /// The settings block found in every save sample of the platform.
    pub fn default_header(self) -> [u8; HEADER_LEN] {
        let mut header = [0; HEADER_LEN];
        match self {
            Platform::Ios => header.copy_from_slice(&[
                0x00, 0x01, 0x04, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xd9, 0x00,
                0x00, 0x00, 0xff, 0x7b, 0x01, 0x18, 0xf0, 0xff, 0xcf, 0x03, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x08, 0x01, 0x00, 0xff, 0x39, 0xfc, 0x07,
            ]),
            Platform::Psp => {
                header[..16].copy_from_slice(&[
                    0x00, 0x03, 0x07, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
                    0x01, 0x00, 0x00,
                ]);
                header[16..].fill(0xff);
            }
        }
        header
    }
}

fn translate(data: &mut [u8], table: &[u8; 256]) {
    for b in data {
        *b = table[*b as usize];
    }
}

fn xor_stream(data: &mut [u8], seed: u32) {
    let mut lo = match seed & 0xFFFF {
        0 => LO_MUL,
        lo => lo,
    };
    let mut hi = match seed >> 16 {
        0 => HI_MUL,
        hi => hi,
    };
    for word in data.as_chunks_mut::<4>().0 {
        lo = lo * LO_MUL % LO_MOD;
        hi = hi * HI_MUL % HI_MOD;
        *word = (u32::from_le_bytes(*word) ^ (lo | hi << 16)).to_le_bytes();
    }
}

pub fn seed_of(plain: &[u8]) -> u32 {
    u32::from_le_bytes(plain[SEED_OFF..BODY_LEN].try_into().unwrap())
}

/// Game-layer ciphertext -> plaintext (with the plain seed at the end).
pub fn decrypt(cipher: &[u8]) -> Vec<u8> {
    assert_eq!(cipher.len(), BODY_LEN);
    let mut out = cipher.to_vec();
    let (body, seed) = out.split_at_mut(SEED_OFF);
    translate(seed, &DEC_TABLE);
    translate(seed, &DEC_TABLE);
    translate(body, &DEC_TABLE);
    xor_stream(body, u32::from_le_bytes(seed.try_into().unwrap()));
    translate(body, &DEC_TABLE);
    out
}

/// Plaintext (with the plain seed at the end) -> game-layer ciphertext.
pub fn encrypt(plain: &[u8]) -> Vec<u8> {
    assert_eq!(plain.len(), BODY_LEN);
    let mut out = plain.to_vec();
    let (body, seed) = out.split_at_mut(SEED_OFF);
    translate(body, &ENC_TABLE);
    xor_stream(body, u32::from_le_bytes((&*seed).try_into().unwrap()));
    translate(body, &ENC_TABLE);
    translate(seed, &ENC_TABLE);
    translate(seed, &ENC_TABLE);
    out
}

pub fn save_hash(plain: &[u8], platform: Platform) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(&plain[..HASHED_LEN]);
    hasher.update(platform.salt());
    hasher.finalize().into()
}

pub fn hash_ok(plain: &[u8], platform: Platform) -> bool {
    plain.len() >= SEED_OFF && save_hash(plain, platform) == plain[HASH_OFF..SEED_OFF]
}

/// A seed for [seal] that the game won't change (halves of 0 would be
/// replaced by the defaults).
pub fn random_seed() -> u32 {
    let r = crate::random_u64();
    let lo = (r % u64::from(LO_MOD - 1)) as u32 + 1;
    let hi = ((r >> 32) % u64::from(HI_MOD - 1)) as u32 + 1;
    lo | hi << 16
}

/// Store the hash and a seed, like the game does when saving.
pub fn seal(plain: &mut [u8], platform: Platform, seed: u32) {
    let hash = save_hash(plain, platform);
    plain[HASH_OFF..SEED_OFF].copy_from_slice(&hash);
    plain[SEED_OFF..BODY_LEN].copy_from_slice(&seed.to_le_bytes());
}

/// The name of the first character slot.
pub fn character_name(plain: &[u8]) -> String {
    let units: Vec<u16> = plain[NAME_OFF..NAME_OFF + NAME_LEN]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
pub use tests::synthetic_plain;

#[cfg(test)]
mod tests {
    use super::*;

    pub fn synthetic_plain(platform: Platform) -> Vec<u8> {
        let mut plain = vec![0; BODY_LEN];
        plain[..HEADER_LEN].copy_from_slice(&platform.default_header());
        for (i, u) in "テスト".encode_utf16().enumerate() {
            plain[NAME_OFF + 2 * i..NAME_OFF + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        // Real saves have another field after the name's terminator.
        plain[NAME_OFF + 8] = 0x6A;
        for (i, b) in plain[0x2000..0x2100].iter_mut().enumerate() {
            *b = i as u8;
        }
        seal(&mut plain, platform, 0x12345678);
        plain
    }

    #[test]
    fn tables_are_inverse() {
        for i in 0..256 {
            assert_eq!(DEC_TABLE[ENC_TABLE[i] as usize] as usize, i);
        }
    }

    #[test]
    fn round_trip() {
        let plain = synthetic_plain(Platform::Ios);
        let cipher = encrypt(&plain);
        assert_ne!(cipher[..16], plain[..16]);
        let mut seed = plain[SEED_OFF..].to_vec();
        translate(&mut seed, &ENC_TABLE);
        translate(&mut seed, &ENC_TABLE);
        assert_eq!(cipher[SEED_OFF..], seed);
        assert_eq!(decrypt(&cipher), plain);
    }

    #[test]
    fn zero_seed_halves_use_defaults() {
        let (mut a, mut b) = ([0u8; 8], [0u8; 8]);
        xor_stream(&mut a, 0);
        xor_stream(&mut b, 0xDFA3215F);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_covers_data_but_not_metadata() {
        let mut plain = synthetic_plain(Platform::Ios);
        assert!(hash_ok(&plain, Platform::Ios));
        assert!(!hash_ok(&plain, Platform::Psp));
        plain[META_OFF] ^= 1;
        assert!(hash_ok(&plain, Platform::Ios));
        plain[0x2000] ^= 1;
        assert!(!hash_ok(&plain, Platform::Ios));
    }

    #[test]
    fn name() {
        assert_eq!(character_name(&synthetic_plain(Platform::Psp)), "テスト");
    }

    #[test]
    fn random_seed_has_no_zero_halves() {
        for _ in 0..100 {
            let seed = random_seed();
            assert!(seed & 0xFFFF != 0 && seed >> 16 != 0);
        }
    }
}
