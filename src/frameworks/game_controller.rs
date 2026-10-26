/*
 * This Source Code Form is subject to the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Bridge host SDL controllers to the iOS 7 Game Controller profiles.

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{ConstantExports, HostConstant, HostDylib};
use crate::frameworks::foundation::{ns_array, ns_string, NSInteger};
use crate::libc::dispatch::{_Block_copy, _Block_release};
use crate::mem::{ConstVoidPtr, MutVoidPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
};
use crate::Environment;
use sdl2::controller::Button;

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

#[derive(Default)]
pub struct State {
    requested: bool,
    updating: bool,
    /// Controllers are retained while their SDL instance is connected.
    controllers: Vec<(u32, id)>,
    discovery_handler: MutVoidPtr,
}

struct Controller {
    name: String,
    attached: bool,
    player_index: NSInteger,
    profiles: [id; 2],
    pause_handler: MutVoidPtr,
    start_pressed: bool,
}
impl HostObject for Controller {}

struct Profile {
    /// Weak: the controller owns its profiles.
    controller: id,
    /// A, B, X, Y, LB, RB, LT, RT, dpad, left stick, right stick.
    elements: [id; 11],
    handler: MutVoidPtr,
}
impl HostObject for Profile {}

#[derive(Default)]
struct Element {
    /// Weak: the parent owns this element.
    parent: id,
    analog: bool,
    value: f32,
    xy: [f32; 2],
    /// X/Y axes, then up/down/left/right buttons for a direction pad.
    children: Vec<id>,
    handler: MutVoidPtr,
    pressed_handler: MutVoidPtr,
}
impl HostObject for Element {}

fn new_element(env: &mut Environment, class: &str, parent: id, analog: bool) -> id {
    let class = env.objc.get_known_class(class, &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(Element {
            parent,
            analog,
            ..Element::default()
        }),
        &mut env.mem,
    )
}

fn new_profile(env: &mut Environment, controller: id, extended: bool) -> id {
    let class = env.objc.get_known_class(
        if extended {
            "GCExtendedGamepad"
        } else {
            "GCGamepad"
        },
        &mut env.mem,
    );
    let profile = env.objc.alloc_object(
        class,
        Box::new(Profile {
            controller,
            elements: [nil; 11],
            handler: MutVoidPtr::null(),
        }),
        &mut env.mem,
    );
    let elements = std::array::from_fn(|i| {
        if i < 8 {
            new_element(env, "GCControllerButtonInput", profile, i >= 6)
        } else {
            let pad = new_element(env, "GCControllerDirectionPad", profile, i != 8);
            let children = (0..6)
                .map(|j| {
                    new_element(
                        env,
                        if j < 2 {
                            "GCControllerAxisInput"
                        } else {
                            "GCControllerButtonInput"
                        },
                        pad,
                        i != 8,
                    )
                })
                .collect();
            env.objc.borrow_mut::<Element>(pad).children = children;
            pad
        }
    });
    env.objc.borrow_mut::<Profile>(profile).elements = elements;
    profile
}

fn new_controller(env: &mut Environment, name: String) -> id {
    let class = env.objc.get_known_class("GCController", &mut env.mem);
    let controller = env.objc.alloc_object(
        class,
        Box::new(Controller {
            name,
            attached: true,
            player_index: -1,
            profiles: [nil; 2],
            pause_handler: MutVoidPtr::null(),
            start_pressed: false,
        }),
        &mut env.mem,
    );
    let profiles = [
        new_profile(env, controller, false),
        new_profile(env, controller, true),
    ];
    env.objc.borrow_mut::<Controller>(controller).profiles = profiles;
    controller
}

fn notify(env: &mut Environment, controller: id, connected: bool) {
    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    let name = ns_string::get_static_str(
        env,
        if connected {
            "GCControllerDidConnectNotification"
        } else {
            "GCControllerDidDisconnectNotification"
        },
    );
    let _: () = msg![env; center postNotificationName:name object:controller];
}

fn invoke(env: &Environment, block: MutVoidPtr) -> GuestFunction {
    GuestFunction::from_addr_with_thumb_bit(env.mem.read(block.cast::<u32>() + 3))
}

/// The block is copied before guest execution: callbacks may replace
/// themselves.
fn profile_callback(env: &mut Environment, profile: id, element: id) {
    let block = env.objc.borrow::<Profile>(profile).handler;
    if block.is_null() {
        return;
    }
    let block = _Block_copy(env, block.cast_const());
    let _: () = invoke(env, block).call_from_host(env, (block, profile, element));
    log_once!("App completed a GameController profile input callback");
    _Block_release(env, block);
}

fn element_callback(env: &mut Environment, element: id, pressed_changed: bool, pad: bool) {
    let object = env.objc.borrow::<Element>(element);
    let (handler, pressed_handler, value, xy) = (
        object.handler,
        object.pressed_handler,
        object.value,
        object.xy,
    );
    let handler = _Block_copy(env, handler.cast_const());
    let pressed_handler = if pressed_changed {
        _Block_copy(env, pressed_handler.cast_const())
    } else {
        MutVoidPtr::null()
    };
    if !handler.is_null() {
        let f = invoke(env, handler);
        if pad {
            let _: () = f.call_from_host(env, (handler, element, xy[0], xy[1]));
        } else if env.objc.borrow::<Element>(element).children.is_empty() {
            // Axis handlers take (axis, value); buttons also take pressed.
            let class: id = msg_class![env; GCControllerAxisInput class];
            let actual_class: id = msg![env; element class];
            let axis = actual_class == class;
            if axis {
                let _: () = f.call_from_host(env, (handler, element, value));
            } else {
                let _: () = f.call_from_host(env, (handler, element, value, value > 0.5));
            }
        }
        _Block_release(env, handler);
    }
    if !pressed_handler.is_null() {
        let _: () = invoke(env, pressed_handler)
            .call_from_host(env, (pressed_handler, element, value, value > 0.5));
        _Block_release(env, pressed_handler);
    }
}

/// Update all values before invoking callbacks, so handlers see one snapshot.
fn update_profile(env: &mut Environment, profile: id, values: &[f32; 14]) {
    let elements = env.objc.borrow::<Profile>(profile).elements;
    let mut changed = Vec::new();
    for (i, &element) in elements.iter().enumerate() {
        if i < 8 {
            let object = env.objc.borrow_mut::<Element>(element);
            let old = object.value;
            object.value = values[i];
            if old != object.value {
                changed.push((element, (old > 0.5) != (object.value > 0.5), false));
            }
        } else {
            let xy = [values[8 + (i - 8) * 2], values[9 + (i - 8) * 2]];
            let object = env.objc.borrow_mut::<Element>(element);
            if object.xy == xy {
                continue;
            }
            object.xy = xy;
            let children = object.children.clone();
            let child_values = [
                xy[0],
                xy[1],
                xy[1].max(0.0),
                (-xy[1]).max(0.0),
                (-xy[0]).max(0.0),
                xy[0].max(0.0),
            ];
            for (child, value) in children.into_iter().zip(child_values) {
                let object = env.objc.borrow_mut::<Element>(child);
                let old = object.value;
                object.value = value;
                if old != value {
                    changed.push((child, (old > 0.5) != (value > 0.5), false));
                }
            }
            changed.push((element, false, true));
        }
    }
    for (element, pressed_changed, pad) in changed {
        element_callback(env, element, pressed_changed, pad);
        // A direction pad is the profile element; its children have their own
        // callbacks, but don't cause duplicate profile notifications.
        if elements.contains(&element) {
            profile_callback(env, profile, element);
        }
    }
}

fn snapshot_values(axes: [f32; 6], buttons: &[Button]) -> [f32; 14] {
    let pressed = |button| if buttons.contains(&button) { 1.0 } else { 0.0 };
    [
        pressed(Button::A),
        pressed(Button::B),
        pressed(Button::X),
        pressed(Button::Y),
        pressed(Button::LeftShoulder),
        pressed(Button::RightShoulder),
        axes[4],
        axes[5],
        pressed(Button::DPadRight) - pressed(Button::DPadLeft),
        pressed(Button::DPadUp) - pressed(Button::DPadDown),
        axes[0],
        -axes[1],
        axes[2],
        -axes[3],
    ]
}

/// Enable polling without callbacks yet: notification observers may be added
/// while the game's controller manager is still being initialized.
pub(crate) fn request_input(env: &mut Environment) {
    if !env.framework_state.game_controller.requested {
        log!("App requested native GameController input");
    }
    env.framework_state.game_controller.requested = true;
    if let Some(window) = env.window.as_mut() {
        window.set_native_controller_input(true);
    }
}

fn activate(env: &mut Environment) {
    request_input(env);
    handle_controllers(env);
}

/// Poll on the main run loop. Games that never request GameController retain
/// the existing touch/tilt mappings instead.
pub fn handle_controllers(env: &mut Environment) {
    if env.current_thread != 0 {
        return;
    }
    let state = &mut env.framework_state.game_controller;
    if !state.requested || state.updating {
        return;
    }
    let Some(window) = env.window.as_ref() else {
        return;
    };
    let snapshots = window.controller_snapshots(env.options.deadzone);
    state.updating = true;
    let disconnected: Vec<_> = state
        .controllers
        .iter()
        .copied()
        .filter(|(instance, _)| !snapshots.iter().any(|s| s.instance_id == *instance))
        .collect();
    for (instance, controller) in disconnected {
        let object = env.objc.borrow_mut::<Controller>(controller);
        object.attached = false;
        object.start_pressed = false;
        let profiles = object.profiles;
        for profile in profiles {
            update_profile(env, profile, &[0.0; 14]);
        }
        env.framework_state
            .game_controller
            .controllers
            .retain(|(i, _)| *i != instance);
        notify(env, controller, false);
        release(env, controller);
    }
    for snapshot in snapshots {
        let controller = match env
            .framework_state
            .game_controller
            .controllers
            .iter()
            .find(|(i, _)| *i == snapshot.instance_id)
        {
            Some(&(_, controller)) => controller,
            None => {
                let controller = new_controller(env, snapshot.name);
                env.framework_state
                    .game_controller
                    .controllers
                    .push((snapshot.instance_id, controller));
                log!("GameController connected: {:?}", controller);
                notify(env, controller, true);
                controller
            }
        };
        let values = snapshot_values(snapshot.axes, &snapshot.buttons);
        let profiles = env.objc.borrow::<Controller>(controller).profiles;
        for profile in profiles {
            update_profile(env, profile, &values);
        }
        let object = env.objc.borrow_mut::<Controller>(controller);
        let start = snapshot.buttons.contains(&Button::Start);
        let pause = start && !object.start_pressed;
        object.start_pressed = start;
        let handler = object.pause_handler;
        if pause && !handler.is_null() {
            let handler = _Block_copy(env, handler.cast_const());
            let _: () = invoke(env, handler).call_from_host(env, (handler, controller));
            _Block_release(env, handler);
        }
    }
    env.framework_state.game_controller.updating = false;
}

const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation GCController: NSObject
+ (id)controllers {
    activate(env);
    let controllers = env.framework_state.game_controller.controllers.iter().map(|&(_, c)| c).collect();
    let controllers: Vec<_> = controllers;
    for &controller in &controllers { retain(env, controller); }
    let array = ns_array::from_vec(env, controllers);
    autorelease(env, array)
}
+ (())startWirelessControllerDiscoveryWithCompletionHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.framework_state.game_controller.discovery_handler, handler);
    _Block_release(env, old);
    activate(env);
}
+ (())stopWirelessControllerDiscovery {
    let handler = std::mem::take(&mut env.framework_state.game_controller.discovery_handler);
    if !handler.is_null() {
        let _: () = invoke(env, handler).call_from_host(env, (handler,));
        _Block_release(env, handler);
    }
}
- (id)vendorName {
    let name = env.objc.borrow::<Controller>(this).name.clone();
    let string = ns_string::from_rust_string(env, name);
    autorelease(env, string)
}
- (bool)isAttached { env.objc.borrow::<Controller>(this).attached }
- (bool)isAttachedToDevice { false }
- (NSInteger)playerIndex { env.objc.borrow::<Controller>(this).player_index }
- (())setPlayerIndex:(NSInteger)index { env.objc.borrow_mut::<Controller>(this).player_index = index; }
- (id)gamepad { env.objc.borrow::<Controller>(this).profiles[0] }
- (id)extendedGamepad { env.objc.borrow::<Controller>(this).profiles[1] }
- (MutVoidPtr)controllerPausedHandler { env.objc.borrow::<Controller>(this).pause_handler }
- (())setControllerPausedHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Controller>(this).pause_handler, handler);
    _Block_release(env, old);
}
- (())dealloc {
    let object = env.objc.borrow::<Controller>(this);
    let (profiles, handler) = (object.profiles, object.pause_handler);
    for profile in profiles {
        env.objc.borrow_mut::<Profile>(profile).controller = nil;
        release(env, profile);
    }
    _Block_release(env, handler);
    env.objc.dealloc_object(this, &mut env.mem);
}
@end

@implementation GCGamepad: NSObject
- (id)controller { env.objc.borrow::<Profile>(this).controller }
- (MutVoidPtr)valueChangedHandler { env.objc.borrow::<Profile>(this).handler }
- (())setValueChangedHandler:(ConstVoidPtr)handler {
    if !handler.is_null() { log_once!("App installed a GameController gamepad callback"); }
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Profile>(this).handler, handler);
    _Block_release(env, old);
}
- (id)buttonA { env.objc.borrow::<Profile>(this).elements[0] }
- (id)buttonB { env.objc.borrow::<Profile>(this).elements[1] }
- (id)buttonX { env.objc.borrow::<Profile>(this).elements[2] }
- (id)buttonY { env.objc.borrow::<Profile>(this).elements[3] }
- (id)leftShoulder { env.objc.borrow::<Profile>(this).elements[4] }
- (id)rightShoulder { env.objc.borrow::<Profile>(this).elements[5] }
- (id)leftTrigger { env.objc.borrow::<Profile>(this).elements[6] }
- (id)rightTrigger { env.objc.borrow::<Profile>(this).elements[7] }
- (id)dpad { env.objc.borrow::<Profile>(this).elements[8] }
- (id)leftThumbstick { env.objc.borrow::<Profile>(this).elements[9] }
- (id)rightThumbstick { env.objc.borrow::<Profile>(this).elements[10] }
- (())dealloc {
    let object = env.objc.borrow::<Profile>(this);
    let (elements, handler) = (object.elements, object.handler);
    for element in elements {
        env.objc.borrow_mut::<Element>(element).parent = nil;
        release(env, element);
    }
    _Block_release(env, handler);
    env.objc.dealloc_object(this, &mut env.mem);
}
@end

@implementation GCExtendedGamepad: NSObject
- (id)controller { env.objc.borrow::<Profile>(this).controller }
- (MutVoidPtr)valueChangedHandler { env.objc.borrow::<Profile>(this).handler }
- (())setValueChangedHandler:(ConstVoidPtr)handler {
    if !handler.is_null() { log_once!("App installed a GameController extended gamepad callback"); }
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Profile>(this).handler, handler);
    _Block_release(env, old);
}
- (id)buttonA { env.objc.borrow::<Profile>(this).elements[0] }
- (id)buttonB { env.objc.borrow::<Profile>(this).elements[1] }
- (id)buttonX { env.objc.borrow::<Profile>(this).elements[2] }
- (id)buttonY { env.objc.borrow::<Profile>(this).elements[3] }
- (id)leftShoulder { env.objc.borrow::<Profile>(this).elements[4] }
- (id)rightShoulder { env.objc.borrow::<Profile>(this).elements[5] }
- (id)leftTrigger { env.objc.borrow::<Profile>(this).elements[6] }
- (id)rightTrigger { env.objc.borrow::<Profile>(this).elements[7] }
- (id)dpad { env.objc.borrow::<Profile>(this).elements[8] }
- (id)leftThumbstick { env.objc.borrow::<Profile>(this).elements[9] }
- (id)rightThumbstick { env.objc.borrow::<Profile>(this).elements[10] }
- (())dealloc {
    let object = env.objc.borrow::<Profile>(this);
    let (elements, handler) = (object.elements, object.handler);
    for element in elements {
        env.objc.borrow_mut::<Element>(element).parent = nil;
        release(env, element);
    }
    _Block_release(env, handler);
    env.objc.dealloc_object(this, &mut env.mem);
}
@end

@implementation GCControllerElement: NSObject
- (id)collection { env.objc.borrow::<Element>(this).parent }
- (bool)isAnalog { env.objc.borrow::<Element>(this).analog }
- (())dealloc {
    let object = env.objc.borrow::<Element>(this);
    let (children, handler, pressed_handler) = (object.children.clone(), object.handler, object.pressed_handler);
    for child in children {
        env.objc.borrow_mut::<Element>(child).parent = nil;
        release(env, child);
    }
    _Block_release(env, handler);
    _Block_release(env, pressed_handler);
    env.objc.dealloc_object(this, &mut env.mem);
}
@end

@implementation GCControllerAxisInput: GCControllerElement
- (f32)value {
    let value = env.objc.borrow::<Element>(this).value;
    if value != 0.0 { log_once!("App read nonzero GameController axis input"); }
    value
}
- (MutVoidPtr)valueChangedHandler { env.objc.borrow::<Element>(this).handler }
- (())setValueChangedHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Element>(this).handler, handler);
    _Block_release(env, old);
}
@end

@implementation GCControllerButtonInput: GCControllerElement
- (f32)value { env.objc.borrow::<Element>(this).value }
- (MutVoidPtr)valueChangedHandler { env.objc.borrow::<Element>(this).handler }
- (())setValueChangedHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Element>(this).handler, handler);
    _Block_release(env, old);
}
- (bool)isPressed { env.objc.borrow::<Element>(this).value > 0.5 }
- (MutVoidPtr)pressedChangedHandler { env.objc.borrow::<Element>(this).pressed_handler }
- (())setPressedChangedHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Element>(this).pressed_handler, handler);
    _Block_release(env, old);
}
@end

@implementation GCControllerDirectionPad: GCControllerElement
- (MutVoidPtr)valueChangedHandler { env.objc.borrow::<Element>(this).handler }
- (())setValueChangedHandler:(ConstVoidPtr)handler {
    let handler = _Block_copy(env, handler);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<Element>(this).handler, handler);
    _Block_release(env, old);
}
- (id)xAxis { env.objc.borrow::<Element>(this).children[0] }
- (id)yAxis { env.objc.borrow::<Element>(this).children[1] }
- (id)up { env.objc.borrow::<Element>(this).children[2] }
- (id)down { env.objc.borrow::<Element>(this).children[3] }
- (id)left { env.objc.borrow::<Element>(this).children[4] }
- (id)right { env.objc.borrow::<Element>(this).children[5] }
@end
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xbox_snapshot_uses_apple_axis_directions_and_independent_triggers() {
        let v = snapshot_values(
            [0.25, -0.75, -0.5, 0.4, 0.2, 0.9],
            &[
                Button::A,
                Button::RightShoulder,
                Button::DPadUp,
                Button::DPadLeft,
            ],
        );
        assert_eq!(&v[..8], &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.2, 0.9]);
        assert_eq!(&v[8..], &[-1.0, 1.0, 0.25, 0.75, -0.5, -0.4]);
    }
    #[test]
    fn opposite_dpad_buttons_cancel_and_release_is_neutral() {
        let v = snapshot_values(
            [0.0; 6],
            &[
                Button::DPadUp,
                Button::DPadDown,
                Button::DPadLeft,
                Button::DPadRight,
            ],
        );
        assert_eq!(v, [0.0; 14]);
        assert_eq!(snapshot_values([0.0; 6], &[]), [0.0; 14]);
    }
}
