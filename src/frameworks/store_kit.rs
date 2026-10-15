/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! StoreKit

mod sk_payment;
mod sk_payment_queue;
pub mod sk_product;

#[derive(Default)]
pub struct State {
    sk_payment_queue: sk_payment_queue::State,
}

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/StoreKit.framework/StoreKit",
    aliases: &[],
    class_exports: &[
        sk_payment::CLASSES,
        sk_payment_queue::CLASSES,
        sk_product::CLASSES,
    ],
    constant_exports: &[],
    function_exports: &[],
};
