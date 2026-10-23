/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `AVPlayer`, `AVPlayerItem`, `AVPlayerLayer` and audio mixes.
//!
//! Videos aren't shown yet: playing an item finishes it right away (after a
//! short delay), like `MPMoviePlayerController` does for movies it can't play.
//! Apps usually wait for this to continue.

use crate::abi::impl_GuestRet_for_large_struct;
use crate::abi::GuestArg;
use crate::frameworks::core_media::{kCMTimeFlags_Valid, CMTime};
use crate::frameworks::foundation::ns_array;
use crate::objc::{
    autorelease, id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports,
    HostObject, NSZonePtr,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    /// `AVPlayer*` of each `AVPlayerLayer*`, retained.
    layer_players: HashMap<id, id>,
}

impl GuestArg for CMTime {
    const REG_COUNT: usize = 6;
    fn from_regs(regs: &[u32]) -> Self {
        CMTime {
            value: GuestArg::from_regs(&regs[0..2]),
            timescale: GuestArg::from_regs(&regs[2..3]),
            flags: GuestArg::from_regs(&regs[3..4]),
            epoch: GuestArg::from_regs(&regs[4..6]),
        }
    }
    fn to_regs(self, regs: &mut [u32]) {
        let CMTime {
            value,
            timescale,
            flags,
            epoch,
        } = self;
        value.to_regs(&mut regs[0..2]);
        timescale.to_regs(&mut regs[2..3]);
        flags.to_regs(&mut regs[3..4]);
        epoch.to_regs(&mut regs[4..6]);
    }
}
impl_GuestRet_for_large_struct!(CMTime);

const ZERO_TIME: CMTime = CMTime {
    value: 0,
    timescale: 1,
    flags: kCMTimeFlags_Valid,
    epoch: 0,
};

struct AVPlayerItemHostObject {
    /// `NSURL*`
    url: id,
    /// `AVAudioMix*`
    audio_mix: id,
}
impl HostObject for AVPlayerItemHostObject {}

struct AVPlayerHostObject {
    /// `AVPlayerItem*`
    current_item: id,
    rate: f32,
    volume: f32,
}
impl HostObject for AVPlayerHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation AVPlayerItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(AVPlayerItemHostObject { url: nil, audio_mix: nil });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)playerItemWithURL:(id)url { // NSURL*
    let item: id = msg![env; this alloc];
    let item: id = msg![env; item initWithURL:url];
    autorelease(env, item)
}

- (id)initWithURL:(id)url { // NSURL*
    log!("[(AVPlayerItem*){:?} initWithURL:{:?}]: videos aren't shown yet", this, url);
    retain(env, url);
    env.objc.borrow_mut::<AVPlayerItemHostObject>(this).url = url;
    this
}

- (())dealloc {
    let &AVPlayerItemHostObject { url, audio_mix } = env.objc.borrow(this);
    release(env, url);
    release(env, audio_mix);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)asset {
    // Assets aren't implemented. Returning nil means e.g. asking for its
    // tracks gives nil.
    nil
}

- (id)audioMix {
    env.objc.borrow::<AVPlayerItemHostObject>(this).audio_mix
}
- (())setAudioMix:(id)audio_mix {
    let audio_mix: id = msg![env; audio_mix copy];
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<AVPlayerItemHostObject>(this).audio_mix,
        audio_mix,
    );
    release(env, old);
}

- (CMTime)duration {
    ZERO_TIME
}
- (CMTime)currentTime {
    ZERO_TIME
}

- (())_touchHLE_playToEnd {
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    let name = crate::frameworks::foundation::ns_string::get_static_str(
        env,
        "AVPlayerItemDidPlayToEndTimeNotification",
    );
    () = msg![env; center postNotificationName:name object:this];
    release(env, this);
}

@end

@implementation AVPlayer: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(AVPlayerHostObject {
        current_item: nil,
        rate: 0.0,
        volume: 1.0,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)playerWithPlayerItem:(id)item {
    let player: id = msg![env; this alloc];
    let player: id = msg![env; player initWithPlayerItem:item];
    autorelease(env, player)
}
+ (id)playerWithURL:(id)url {
    let player: id = msg![env; this alloc];
    let player: id = msg![env; player initWithURL:url];
    autorelease(env, player)
}

- (id)init {
    this
}
- (id)initWithPlayerItem:(id)item {
    retain(env, item);
    env.objc.borrow_mut::<AVPlayerHostObject>(this).current_item = item;
    this
}
- (id)initWithURL:(id)url {
    let item: id = msg_class![env; AVPlayerItem playerItemWithURL:url];
    msg![env; this initWithPlayerItem:item]
}

- (())dealloc {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    release(env, item);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)currentItem {
    env.objc.borrow::<AVPlayerHostObject>(this).current_item
}
- (())replaceCurrentItemWithPlayerItem:(id)item {
    retain(env, item);
    let host_object = env.objc.borrow_mut::<AVPlayerHostObject>(this);
    let old = std::mem::replace(&mut host_object.current_item, item);
    host_object.rate = 0.0;
    release(env, old);
}

- (())play {
    let item = env.objc.borrow::<AVPlayerHostObject>(this).current_item;
    log!("[(AVPlayer*){:?} play]: videos aren't shown yet, finishing {:?}", this, item);
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 1.0;
    if item != nil {
        // Released by _touchHLE_playToEnd.
        retain(env, item);
        let sel = env.objc.lookup_selector("_touchHLE_playToEnd").unwrap();
        () = msg![env; item performSelector:sel withObject:nil afterDelay:0.5f64];
    }
}
- (())pause {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = 0.0;
}
- (f32)rate {
    env.objc.borrow::<AVPlayerHostObject>(this).rate
}
- (())setRate:(f32)rate {
    if rate != 0.0 {
        () = msg![env; this play];
    }
    env.objc.borrow_mut::<AVPlayerHostObject>(this).rate = rate;
}
- (f32)volume {
    env.objc.borrow::<AVPlayerHostObject>(this).volume
}
- (())setVolume:(f32)volume {
    env.objc.borrow_mut::<AVPlayerHostObject>(this).volume = volume;
}

- (CMTime)currentTime {
    ZERO_TIME
}
- (())seekToTime:(CMTime)_time {
}
- (())seekToTime:(CMTime)_time
  toleranceBefore:(CMTime)_before
   toleranceAfter:(CMTime)_after {
}

@end

@implementation AVPlayerLayer: CALayer

+ (id)playerLayerWithPlayer:(id)player {
    let layer: id = msg![env; this new];
    () = msg![env; layer setPlayer:player];
    autorelease(env, layer)
}

- (id)player {
    env.framework_state
        .avfoundation
        .av_player
        .layer_players
        .get(&this)
        .copied()
        .unwrap_or(nil)
}
- (())setPlayer:(id)player {
    retain(env, player);
    let old = env
        .framework_state
        .avfoundation
        .av_player
        .layer_players
        .insert(this, player);
    if let Some(old) = old {
        release(env, old);
    }
}

- (id)videoGravity {
    crate::frameworks::foundation::ns_string::get_static_str(env, "AVLayerVideoGravityResizeAspect")
}
- (())setVideoGravity:(id)_gravity {
}

- (())dealloc {
    let player = env
        .framework_state
        .avfoundation
        .av_player
        .layer_players
        .remove(&this);
    if let Some(player) = player {
        release(env, player);
    }
    msg_super![env; this dealloc]
}

@end

// Audio mixes are accepted but have no effect.
@implementation AVAudioMix: NSObject
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}
- (id)inputParameters {
    let array = ns_array::from_vec(env, Vec::new());
    autorelease(env, array)
}
@end

@implementation AVMutableAudioMix: AVAudioMix
+ (id)audioMix {
    let mix: id = msg![env; this new];
    autorelease(env, mix)
}
- (())setInputParameters:(id)_parameters {
}
@end

@implementation AVAudioMixInputParameters: NSObject
@end

@implementation AVMutableAudioMixInputParameters: AVAudioMixInputParameters
+ (id)audioMixInputParameters {
    let parameters: id = msg![env; this new];
    autorelease(env, parameters)
}
+ (id)audioMixInputParametersWithTrack:(id)_track {
    let parameters: id = msg![env; this new];
    autorelease(env, parameters)
}
- (())setVolume:(f32)_volume atTime:(CMTime)_time {
}
- (())setTrackID:(i32)_track_id {
}
@end

};
