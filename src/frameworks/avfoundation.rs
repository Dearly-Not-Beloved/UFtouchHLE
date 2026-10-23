/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! The AVFoundation framework.

mod av_audio_player;
mod av_audio_session;
mod av_player;

use crate::dyld::{ConstantExports, HostConstant};

pub const CONSTANTS: ConstantExports = &[
    (
        "_AVLayerVideoGravityResize",
        HostConstant::NSString("AVLayerVideoGravityResize"),
    ),
    (
        "_AVLayerVideoGravityResizeAspect",
        HostConstant::NSString("AVLayerVideoGravityResizeAspect"),
    ),
    ("_AVMediaTypeAudio", HostConstant::NSString("soun")),
    (
        "_AVPlayerItemDidPlayToEndTimeNotification",
        HostConstant::NSString("AVPlayerItemDidPlayToEndTimeNotification"),
    ),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/AVFoundation.framework/AVFoundation",
    aliases: &[],
    class_exports: &[
        av_audio_player::CLASSES,
        av_audio_session::CLASSES,
        av_player::CLASSES,
    ],
    constant_exports: &[av_audio_session::CONSTANTS, CONSTANTS],
    function_exports: &[],
};

#[derive(Default)]
pub struct State {
    av_audio_session: av_audio_session::State,
    av_player: av_player::State,
}
