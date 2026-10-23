/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIBezierPath`.
//!
//! Paths aren't really supported: only their bounding box is kept, and filling
//! a path fills that box (so rounded rectangles get square corners). Strokes
//! aren't drawn.

use crate::frameworks::core_graphics::cg_context::CGContextFillRect;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::uikit::ui_graphics::UIGraphicsGetCurrentContext;
use crate::objc::{autorelease, id, msg, nil, objc_classes, ClassExports, HostObject, NSZonePtr};

#[derive(Default)]
struct UIBezierPathHostObject {
    /// Bounding box as (min x, min y, max x, max y).
    bounds: Option<(CGFloat, CGFloat, CGFloat, CGFloat)>,
}
impl HostObject for UIBezierPathHostObject {}

fn add_point(env: &mut crate::Environment, this: id, point: CGPoint) {
    let host_object = env.objc.borrow_mut::<UIBezierPathHostObject>(this);
    host_object.bounds = Some(match host_object.bounds {
        None => (point.x, point.y, point.x, point.y),
        Some((x0, y0, x1, y1)) => (
            x0.min(point.x),
            y0.min(point.y),
            x1.max(point.x),
            y1.max(point.y),
        ),
    });
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIBezierPath: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIBezierPathHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)bezierPath {
    let path: id = msg![env; this new];
    autorelease(env, path)
}
+ (id)bezierPathWithRect:(CGRect)rect {
    let path: id = msg![env; this new];
    add_point(env, path, rect.origin);
    add_point(env, path, CGPoint {
        x: rect.origin.x + rect.size.width,
        y: rect.origin.y + rect.size.height,
    });
    autorelease(env, path)
}
+ (id)bezierPathWithRoundedRect:(CGRect)rect
                   cornerRadius:(CGFloat)_radius {
    msg![env; this bezierPathWithRect:rect]
}

- (())moveToPoint:(CGPoint)point {
    add_point(env, this, point);
}
- (())addLineToPoint:(CGPoint)point {
    add_point(env, this, point);
}
- (())closePath {
}
- (())setLineWidth:(CGFloat)_width {
}

- (CGRect)bounds {
    let (x0, y0, x1, y1) = env
        .objc
        .borrow::<UIBezierPathHostObject>(this)
        .bounds
        .unwrap_or((0.0, 0.0, 0.0, 0.0));
    CGRect {
        origin: CGPoint { x: x0, y: y0 },
        size: CGSize { width: x1 - x0, height: y1 - y0 },
    }
}

- (id)CGPath {
    // TODO: CGPath isn't implemented
    nil
}

- (())fill {
    let context = UIGraphicsGetCurrentContext(env);
    if context.is_null() {
        return;
    }
    let bounds: CGRect = msg![env; this bounds];
    CGContextFillRect(env, context, bounds);
}
- (())stroke {
    // TODO
}

@end

};
