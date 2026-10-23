/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CAGradientLayer`.
//!
//! Gradients aren't drawn yet: the layer is filled with the gradient's first
//! color, which is usually a reasonable approximation.

use crate::frameworks::core_graphics::cg_color::CGColorRef;
use crate::frameworks::core_graphics::CGPoint;
use crate::frameworks::foundation::NSUInteger;
use crate::objc::{id, msg, nil, objc_classes, todo_objc_setter, ClassExports};

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CAGradientLayer: CALayer

- (())setColors:(id)colors { // NSArray* of CGColorRef
    let count: NSUInteger = if colors == nil { 0 } else { msg![env; colors count] };
    if count > 0 {
        let first: CGColorRef = msg![env; colors objectAtIndex:(0 as NSUInteger)];
        () = msg![env; this setBackgroundColor:first];
    }
}
- (())setLocations:(id)locations { // NSArray* of NSNumber*
    todo_objc_setter!(this, locations);
}
- (())setStartPoint:(CGPoint)point {
    todo_objc_setter!(this, point);
}
- (())setEndPoint:(CGPoint)point {
    todo_objc_setter!(this, point);
}
- (())setType:(id)type_ { // NSString*
    todo_objc_setter!(this, type_);
}

@end

};
