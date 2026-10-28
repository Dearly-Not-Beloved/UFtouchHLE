/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIToolbar`.
//!
//! The bar itself isn't drawn yet. Items with a custom view show that view
//! as a subview, so app-made buttons stay visible and touchable.

use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_super, nil, objc_classes, release, ClassExports,
    NSZonePtr,
};

type UIBarStyle = NSInteger;

#[derive(Default)]
struct UIToolbarHostObject {
    superclass: super::UIViewHostObject,
    /// `NSArray*` of `UIBarButtonItem*`
    items: id,
    bar_style: UIBarStyle,
}
impl_HostObject_with_superclass!(UIToolbarHostObject);

/// The custom views of the items in an array of `UIBarButtonItem*`.
fn custom_views(env: &mut crate::Environment, items: id) -> Vec<id> {
    if items == nil {
        return Vec::new();
    }
    let count: NSUInteger = msg![env; items count];
    (0..count)
        .map(|i| -> id {
            let item: id = msg![env; items objectAtIndex:i];
            msg![env; item customView]
        })
        .filter(|view| *view != nil)
        .collect()
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIToolbar: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIToolbarHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let items = env.objc.borrow::<UIToolbarHostObject>(this).items;
    release(env, items);
    msg_super![env; this dealloc]
}

- (id)items {
    env.objc.borrow::<UIToolbarHostObject>(this).items
}
- (())setItems:(id)items { // NSArray*
    let old_items: id = msg![env; this items];
    for view in custom_views(env, old_items) {
        () = msg![env; view removeFromSuperview];
    }
    let copy: id = if items == nil { nil } else { msg![env; items copy] };
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UIToolbarHostObject>(this).items, copy);
    release(env, old);
    for view in custom_views(env, copy) {
        () = msg![env; this addSubview:view];
    }
}
- (())setItems:(id)items animated:(bool)_animated {
    () = msg![env; this setItems:items];
}

- (UIBarStyle)barStyle {
    env.objc.borrow::<UIToolbarHostObject>(this).bar_style
}
- (())setBarStyle:(UIBarStyle)style {
    env.objc.borrow_mut::<UIToolbarHostObject>(this).bar_style = style;
}

- (())setTintColor:(id)_color { // UIColor*
    // The bar isn't drawn yet.
}
- (())setTranslucent:(bool)_translucent {
    // The bar isn't drawn yet.
}

@end

};
