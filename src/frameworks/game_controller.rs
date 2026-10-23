/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The Game Controller framework.
//!
//! No controllers are ever connected. (touchHLE maps host game controllers to
//! touches and the accelerometer instead, see `--button-to-touch=` etc.)

use crate::dyld::{ConstantExports, HostConstant, HostDylib};
use crate::frameworks::foundation::ns_array;
use crate::objc::{autorelease, id, objc_classes, ClassExports};

pub const DYLIB: HostDylib = HostDylib {
    path: "/System/Library/Frameworks/GameController.framework/GameController",
    aliases: &[],
    class_exports: &[CLASSES],
    constant_exports: &[CONSTANTS],
    function_exports: &[],
};

const CONSTANTS: ConstantExports = &[
    (
        "_GCControllerDidConnectNotification",
        HostConstant::NSString("GCControllerDidConnectNotification"),
    ),
    (
        "_GCControllerDidDisconnectNotification",
        HostConstant::NSString("GCControllerDidDisconnectNotification"),
    ),
];

const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation GCController: NSObject

+ (id)controllers {
    let controllers = ns_array::from_vec(env, Vec::new());
    autorelease(env, controllers)
}

+ (())startWirelessControllerDiscoveryWithCompletionHandler:(id)_handler {
    // There's nothing to discover. The completion handler is only called when
    // discovery is stopped.
}

+ (())stopWirelessControllerDiscovery {
}

@end

};
