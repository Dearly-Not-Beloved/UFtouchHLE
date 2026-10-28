/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! iOS 4.1 NSString-playerID API used by the MHP2G sample.
use super::local_network;
use crate::mem::MutPtr;
use crate::objc::{id, nil, objc_classes, ClassExports, HostObject, NSZonePtr};

pub(super) struct Match {
    pub delegate: id,
    pub disconnected: bool,
}
impl HostObject for Match {}
pub const CLASSES: ClassExports = objc_classes! {
(env,this,_cmd);
@implementation GKMatch: NSObject
+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this,Box::new(Match{delegate:nil,disconnected:false}),&mut env.mem)
}
- (())dealloc {env.objc.dealloc_object(this,&mut env.mem);}
- (id)delegate {env.objc.borrow::<Match>(this).delegate}
- (())setDelegate:(id)delegate {env.objc.borrow_mut::<Match>(this).delegate=delegate;}
- (id)playerIDs {local_network::player_ids(env,this)}
- (u32)expectedPlayerCount {local_network::expected(env,this)}
- (())disconnect {local_network::disconnect(env,this);}
- (bool)sendData:(id)data toPlayers:(id)players withDataMode:(u32)mode error:(MutPtr<id>)out_error {
    let sent=local_send(env,this,data,Some(players),mode);result(env,out_error,sent)
}
- (bool)sendDataToAllPlayers:(id)data withDataMode:(u32)mode error:(MutPtr<id>)out_error {
    let sent=local_send(env,this,data,None,mode);result(env,out_error,sent)
}
@end
};
fn local_send(
    env: &mut crate::Environment,
    match_: id,
    data: id,
    players: Option<id>,
    mode: u32,
) -> Result<(), (i32, String)> {
    local_network::send(env, match_, data, players, mode)
}
fn result(
    env: &mut crate::Environment,
    out: MutPtr<id>,
    result: Result<(), (i32, String)>,
) -> bool {
    let success = result.is_ok();
    let error = match result {
        Ok(()) => nil,
        Err((code, reason)) => local_network::error(env, code, &reason),
    };
    if !out.is_null() {
        env.mem.write(out, error);
    }
    success
}
