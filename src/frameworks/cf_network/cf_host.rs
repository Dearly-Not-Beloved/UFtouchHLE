/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFHost` and the related `CFStream` functions.
//!
//! Connecting to servers isn't supported (the servers old apps used are
//! mostly gone anyway), so these fail in the same way they would without a
//! network connection, which apps generally handle.

use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::cf_allocator::CFAllocatorRef;
use crate::frameworks::core_foundation::cf_string::CFStringRef;
use crate::frameworks::core_foundation::CFTypeRef;
use crate::frameworks::foundation::ns_string;
use crate::mem::{MutPtr, Ptr};
use crate::Environment;

type CFHostRef = CFTypeRef;
type CFDataRef = CFTypeRef;
type CFReadStreamRef = CFTypeRef;
type CFWriteStreamRef = CFTypeRef;

fn CFHostCreateWithName(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    hostname: CFStringRef,
) -> CFHostRef {
    log!(
        "TODO: CFHostCreateWithName(_, {:?}): networking isn't supported, returning NULL",
        ns_string::to_rust_string(env, hostname)
    );
    Ptr::null()
}

fn CFHostCreateWithAddress(
    _env: &mut Environment,
    _allocator: CFAllocatorRef,
    address: CFDataRef,
) -> CFHostRef {
    log!(
        "TODO: CFHostCreateWithAddress(_, {:?}): networking isn't supported, returning NULL",
        address
    );
    Ptr::null()
}

fn CFStreamCreatePairWithSocketToCFHost(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    host: CFHostRef,
    port: i32,
    read_stream: MutPtr<CFReadStreamRef>,
    write_stream: MutPtr<CFWriteStreamRef>,
) {
    log!(
        "TODO: CFStreamCreatePairWithSocketToCFHost(_, {:?}, {}, ...): networking isn't supported",
        host,
        port
    );
    // The streams are set to NULL on failure.
    if !read_stream.is_null() {
        env.mem.write(read_stream, Ptr::null());
    }
    if !write_stream.is_null() {
        env.mem.write(write_stream, Ptr::null());
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFHostCreateWithName(_, _)),
    export_c_func!(CFHostCreateWithAddress(_, _)),
    export_c_func!(CFStreamCreatePairWithSocketToCFHost(_, _, _, _, _)),
];
