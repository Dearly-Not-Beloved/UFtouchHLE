/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The Security framework.
//!
//! Only the keychain item functions, which apps use to store small secrets
//! (e.g. an account ID). The keychain is always empty: adding items succeeds
//! but nothing is stored.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant, HostDylib};
use crate::frameworks::core_foundation::CFTypeRef;
use crate::mem::MutPtr;
use crate::objc::nil;
use crate::Environment;

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/Security.framework/Security",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};

type OSStatus = i32;
const errSecSuccess: OSStatus = 0;
const errSecItemNotFound: OSStatus = -25300;

/// `CFDictionaryRef`
type CFDictionaryRef = CFTypeRef;

fn SecItemAdd(
    _env: &mut Environment,
    attributes: CFDictionaryRef,
    result: MutPtr<CFTypeRef>,
) -> OSStatus {
    log!(
        "TODO: SecItemAdd({:?}, {:?}): not storing anything",
        attributes,
        result
    );
    errSecSuccess
}

fn SecItemCopyMatching(
    env: &mut Environment,
    query: CFDictionaryRef,
    result: MutPtr<CFTypeRef>,
) -> OSStatus {
    log!(
        "SecItemCopyMatching({:?}, {:?}): the keychain is empty",
        query,
        result
    );
    if !result.is_null() {
        env.mem.write(result, nil);
    }
    errSecItemNotFound
}

fn SecItemUpdate(
    _env: &mut Environment,
    query: CFDictionaryRef,
    attributes: CFDictionaryRef,
) -> OSStatus {
    log!(
        "SecItemUpdate({:?}, {:?}): the keychain is empty",
        query,
        attributes
    );
    errSecItemNotFound
}

fn SecItemDelete(_env: &mut Environment, query: CFDictionaryRef) -> OSStatus {
    log!("SecItemDelete({:?}): the keychain is empty", query);
    errSecItemNotFound
}

const CONSTANTS: ConstantExports = &[
    ("_kSecClass", HostConstant::NSString("class")),
    ("_kSecClassGenericPassword", HostConstant::NSString("genp")),
    ("_kSecAttrService", HostConstant::NSString("svce")),
    ("_kSecAttrAccount", HostConstant::NSString("acct")),
    ("_kSecAttrGeneric", HostConstant::NSString("gena")),
    ("_kSecReturnData", HostConstant::NSString("r_Data")),
    ("_kSecValueData", HostConstant::NSString("v_Data")),
    ("_kSecMatchLimit", HostConstant::NSString("m_Limit")),
    ("_kSecMatchLimitOne", HostConstant::NSString("m_LimitOne")),
];

const FUNCTIONS: FunctionExports = &[
    export_c_func!(SecItemAdd(_, _)),
    export_c_func!(SecItemCopyMatching(_, _)),
    export_c_func!(SecItemUpdate(_, _)),
    export_c_func!(SecItemDelete(_)),
];
