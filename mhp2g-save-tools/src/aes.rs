/*
 * mhp2g_save: Monster Hunter Portable 2nd G save converter
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version. See COPYING.
 */
//! Single AES-128 blocks.

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;

pub fn encrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    Aes128::new(GenericArray::from_slice(key)).encrypt_block(GenericArray::from_mut_slice(block));
}

pub fn decrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    Aes128::new(GenericArray::from_slice(key)).decrypt_block(GenericArray::from_mut_slice(block));
}
