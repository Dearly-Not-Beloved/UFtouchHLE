/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CAEAGLLayer`.

use super::ca_layer::CALayerHostObject;
use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::objc::{id, msg, msg_class, nil, objc_classes, Class, ClassExports};
use crate::Environment;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CAEAGLLayer: CALayer

// EAGLDrawable implementation (the only one)

- (id)drawableProperties {
    // FIXME: do we need to return an empty dictionary rather than nil?
    env.objc.borrow::<CALayerHostObject>(this).drawable_properties
}

- (())setDrawableProperties:(id)props { // NSDictionary<NSString*, id>*
    let props: id = msg![env; props copy];
    env.objc.borrow_mut::<CALayerHostObject>(this).drawable_properties = props;
}

@end

};

/// If there is an opaque `CAEAGLLayer` that covers the entire screen, this
/// returns a pointer to it. Otherwise, it returns [nil].
///
/// To avoid a state management nightmare, we want to have an internal OpenGL ES
/// context for compositing, separate from any OpenGL ES contexts the app uses
/// for its rendering. When we have a `CAEAGLLayer` though, we need to transfer
/// a rendered frame from the app's context to the compositor's context, and
/// unfortunately the most practical way to do this is `glReadPixels()`, which
/// is highly inefficient. To make things efficient, then, we have a shortcut:
/// if the result of composition would be identical to the rendered frame, i.e.
/// there's a single full-screen layer, we skip transferring between contexts
/// and present it directly from the app's context. This function is used to
/// determine when that will happen.
pub fn find_fullscreen_eagl_layer(env: &mut Environment) -> id {
    find_fullscreen_eagl_layer_and_rotation(env).0
}

/// If `transform` is a rotation by a whole number of quarter turns (e.g. a
/// landscape app's view rotated to cover the portrait screen), returns that
/// number (0 to 3, clockwise on screen).
fn quarter_turns(transform: &CGAffineTransform) -> Option<u8> {
    const EPSILON: f32 = 1e-5;
    let close = |a: f32, b: f32| (a - b).abs() < EPSILON;
    let &CGAffineTransform { a, b, c, d, tx, ty } = transform;
    if !close(tx, 0.0) || !close(ty, 0.0) {
        return None;
    }
    [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
        .iter()
        .position(|&(cos, sin)| close(a, cos) && close(b, sin) && close(c, -sin) && close(d, cos))
        .map(|turns| turns as u8)
}

/// Like [find_fullscreen_eagl_layer], but also allows the layers to be
/// rotated by quarter turns, in which case the frame has to be rotated when
/// presenting it. Returns the layer (or nil) and the total number of quarter
/// turns (clockwise on screen).
pub fn find_fullscreen_eagl_layer_and_rotation(env: &mut Environment) -> (id, u8) {
    if env.options.force_composition {
        return (nil, 0);
    }

    let windows = env.framework_state.uikit.ui_view.ui_window.windows.clone();
    // Assumes the windows in the list are ordered back-to-front.
    // TODO: this may not be correct once we support windowLevel.
    let Some(top_window) = windows
        .into_iter()
        .rev()
        .find(|&window| !msg![env; window isHidden])
    else {
        return (nil, 0);
    };

    let screen_bounds: CGRect = {
        let screen: id = msg_class![env; UIScreen mainScreen];
        msg![env; screen bounds]
    };

    let mut layer: id = msg![env; top_window layer];
    // Size of the superlayer's bounds (for the window's layer, the screen).
    let mut parent_size = screen_bounds.size;
    let mut total_turns = 0u8;

    // Descend through the hierarchy, looking only at the last visible layer in
    // each list of children, since that should be the one on top.
    // TODO: this is not correct once we support zPosition.
    loop {
        assert!(layer != nil);

        let layer_host_obj: &CALayerHostObject = env.objc.borrow(layer);

        // This is stricter than it should be. In theory we should accumulate
        // the transforms and handle different anchor points etc, but real apps
        // probably only use these common cases: no transform, or a rotation
        // by quarter turns (with the bounds' width and height swapped, so the
        // layer still covers the screen exactly).
        let Some(turns) = quarter_turns(&layer_host_obj.affine_transform) else {
            return (nil, 0);
        };
        let expected_size = if turns % 2 == 1 {
            CGSize {
                width: parent_size.height,
                height: parent_size.width,
            }
        } else {
            parent_size
        };
        if layer_host_obj.bounds.size != expected_size
            || layer_host_obj.bounds.origin != (CGPoint { x: 0.0, y: 0.0 })
            || layer_host_obj.anchor_point != (CGPoint { x: 0.5, y: 0.5 })
            || layer_host_obj.position
                != (CGPoint {
                    x: parent_size.width / 2.0,
                    y: parent_size.height / 2.0,
                })
            || layer_host_obj.hidden
            || layer_host_obj.opacity != 1.0
        {
            return (nil, 0);
        }
        total_turns = (total_turns + turns) % 4;
        parent_size = expected_size;

        // Layers that are hidden or fully transparent don't affect what's
        // shown (apps often keep e.g. a hidden menu above their OpenGL view),
        // so they're skipped.
        let next = layer_host_obj
            .sublayers
            .iter()
            .rev()
            .copied()
            .find(|&sublayer| {
                let sublayer = env.objc.borrow::<CALayerHostObject>(sublayer);
                !sublayer.hidden && sublayer.opacity > 0.0
            });
        if let Some(next) = next {
            layer = next;
        } else {
            break;
        }
    }

    if !env.objc.borrow::<CALayerHostObject>(layer).opaque {
        return (nil, 0);
    }

    let ca_eagl_layer_class: Class = msg_class![env; CAEAGLLayer class];
    if !msg![env; layer isKindOfClass:ca_eagl_layer_class] {
        return (nil, 0);
    }

    (layer, total_turns)
}

/// For use by `EAGLContext` when presenting to a `CAEAGLLayer`:
/// [std::mem::take]s the buffer used to hold the pixels. It should be passed
/// back to [present_pixels] once it has been filled.
pub fn get_pixels_vec_for_presenting(env: &mut Environment, layer: id) -> Vec<u8> {
    env.objc
        .borrow_mut::<CALayerHostObject>(layer)
        .presented_pixels
        .take()
        .map(|(vec, _width, _height)| vec)
        .unwrap_or_default()
}

/// For use by `EAGLContext` when presenting to a `CAEAGLLayer`: provide the new
/// frame rendered by the app, so it can be used when compositing. The buffer
/// should have been obtained with [get_pixels_vec_for_presenting] before
/// filling. The data must be in RGBA8 format.
pub fn present_pixels(env: &mut Environment, layer: id, pixels: Vec<u8>, width: u32, height: u32) {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(layer);
    host_obj.presented_pixels = Some((pixels, width, height));
    host_obj.gles_texture_is_up_to_date = false;
}
