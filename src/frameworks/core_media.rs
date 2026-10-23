/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The Core Media framework.

use crate::dyld::{ConstantExports, HostConstant, HostDylib};
use crate::mem::SafeRead;

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/CoreMedia.framework/CoreMedia",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[],
};

#[allow(non_camel_case_types)]
pub type CMTimeValue = i64;
pub type CMTimeScale = i32;
pub type CMTimeFlags = u32;
pub type CMTimeEpoch = i64;

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct CMTime {
    pub value: CMTimeValue,
    pub timescale: CMTimeScale,
    pub flags: CMTimeFlags,
    pub epoch: CMTimeEpoch,
}
unsafe impl SafeRead for CMTime {}

pub const kCMTimeFlags_Valid: CMTimeFlags = 1;

const CONSTANTS: ConstantExports = &[(
    "_kCMTimeZero",
    HostConstant::Custom(|env| {
        env.mem
            .alloc_and_write(CMTime {
                value: 0,
                timescale: 1,
                flags: kCMTimeFlags_Valid,
                epoch: 0,
            })
            .cast_void()
            .cast_const()
    }),
)];
