/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `GKLocalPlayer`.

use super::local_network;
use crate::dyld::{ConstantExports, HostConstant};
use crate::objc::{id, msg, nil, objc_classes, ClassExports};

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation GKPlayer: NSObject
@end

@implementation GKLocalPlayer: GKPlayer

+ (id)localPlayer {
    if !local_network::enabled(env) {return nil;}
    if let Some(player)=env.framework_state.game_kit.local.local_player {return player;}
    let player:id=msg![env; this new];
    env.framework_state.game_kit.local.local_player=Some(player);
    player
}

- (id)playerID {
    let identity=local_network::identity(env);
    let string=crate::frameworks::foundation::ns_string::from_rust_string(env,local_network::alias(identity));
    crate::objc::autorelease(env,string)
}
- (bool)isAuthenticated {env.framework_state.game_kit.local.authenticated}
- (id)authenticateHandler {env.framework_state.game_kit.local.auth_handler.cast()}
- (())setAuthenticateHandler:(id)handler {local_network::set_auth(env,handler);}

@end

};

pub const GKPlayerAuthenticationDidChangeNotificationName: &str =
    "GKPlayerAuthenticationDidChangeNotificationName";

/// `NSNotificationName` values.
pub const CONSTANTS: ConstantExports = &[
    ("_GKErrorDomain", HostConstant::NSString("GKErrorDomain")),
    (
        "_GKPlayerAuthenticationDidChangeNotificationName",
        HostConstant::NSString(GKPlayerAuthenticationDidChangeNotificationName),
    ),
];
