/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `GKMatchRequest` and `GKMatchmaker`.
//!
//! There is no Game Center, so matchmaking never finds a match.

use crate::frameworks::foundation::NSUInteger;
use crate::objc::{id, msg, objc_classes, release, ClassExports, HostObject, NSZonePtr};

#[derive(Default)]
struct GKMatchRequestHostObject {
    min_players: NSUInteger,
    max_players: NSUInteger,
    default_number_of_players: NSUInteger,
    player_group: NSUInteger,
    player_attributes: u32,
    /// `NSArray*`
    players_to_invite: id,
}
impl HostObject for GKMatchRequestHostObject {}

#[derive(Default)]
pub struct State {
    shared_matchmaker: Option<id>,
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation GKMatchRequest: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(GKMatchRequestHostObject {
        min_players: 2,
        max_players: 4,
        ..Default::default()
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let players_to_invite = env.objc.borrow::<GKMatchRequestHostObject>(this).players_to_invite;
    release(env, players_to_invite);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (NSUInteger)minPlayers {
    env.objc.borrow::<GKMatchRequestHostObject>(this).min_players
}
- (())setMinPlayers:(NSUInteger)value {
    env.objc.borrow_mut::<GKMatchRequestHostObject>(this).min_players = value;
}
- (NSUInteger)maxPlayers {
    env.objc.borrow::<GKMatchRequestHostObject>(this).max_players
}
- (())setMaxPlayers:(NSUInteger)value {
    env.objc.borrow_mut::<GKMatchRequestHostObject>(this).max_players = value;
}
- (NSUInteger)defaultNumberOfPlayers {
    env.objc.borrow::<GKMatchRequestHostObject>(this).default_number_of_players
}
- (())setDefaultNumberOfPlayers:(NSUInteger)value {
    env.objc.borrow_mut::<GKMatchRequestHostObject>(this).default_number_of_players = value;
}
- (NSUInteger)playerGroup {
    env.objc.borrow::<GKMatchRequestHostObject>(this).player_group
}
- (())setPlayerGroup:(NSUInteger)value {
    env.objc.borrow_mut::<GKMatchRequestHostObject>(this).player_group = value;
}
- (u32)playerAttributes {
    env.objc.borrow::<GKMatchRequestHostObject>(this).player_attributes
}
- (())setPlayerAttributes:(u32)value {
    env.objc.borrow_mut::<GKMatchRequestHostObject>(this).player_attributes = value;
}
- (id)playersToInvite {
    env.objc.borrow::<GKMatchRequestHostObject>(this).players_to_invite
}
- (())setPlayersToInvite:(id)players { // NSArray*
    let players: id = msg![env; players copy];
    let host_object = env.objc.borrow_mut::<GKMatchRequestHostObject>(this);
    let old = std::mem::replace(&mut host_object.players_to_invite, players);
    release(env, old);
}

@end

@implementation GKMatchmaker: NSObject

+ (id)sharedMatchmaker {
    if let Some(matchmaker) = env.framework_state.game_kit.gk_matchmaker.shared_matchmaker {
        return matchmaker;
    }
    let matchmaker: id = msg![env; this new];
    env.framework_state.game_kit.gk_matchmaker.shared_matchmaker = Some(matchmaker);
    matchmaker
}

- (id)retain { this }
- (())release {}
- (id)autorelease { this }

- (())findMatchForRequest:(id)request
    withCompletionHandler:(id)_handler { // block
    // The handler is never called, as if matchmaking never finished.
    log!("TODO: [GKMatchmaker findMatchForRequest:{:?} withCompletionHandler:]: there's no Game Center", request);
}
- (())addPlayersToMatch:(id)match_
           matchRequest:(id)request
      completionHandler:(id)_handler { // block
    log!("TODO: [GKMatchmaker addPlayersToMatch:{:?} matchRequest:{:?} completionHandler:]: there's no Game Center", match_, request);
}
- (())finishMatchmakingForMatch:(id)_match {
}
- (())cancel {
}

@end

};
