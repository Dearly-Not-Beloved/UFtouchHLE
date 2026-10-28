/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UINavigationItem`.
//!
//! This only stores what view controllers put in their navigation items. No
//! navigation bar is drawn from it yet.

use crate::objc::{
    id, msg, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};

#[derive(Default)]
struct UINavigationItemHostObject {
    /// `NSString*`, copied
    title: id,
    /// `UIView*`
    title_view: id,
    /// `UIBarButtonItem*`
    left_bar_button_item: id,
    /// `UIBarButtonItem*`
    right_bar_button_item: id,
    /// `UIBarButtonItem*`
    back_bar_button_item: id,
    hides_back_button: bool,
}
impl HostObject for UINavigationItemHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UINavigationItem: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UINavigationItemHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithTitle:(id)title { // NSString*
    () = msg![env; this setTitle:title];
    this
}

- (())dealloc {
    let &UINavigationItemHostObject {
        title,
        title_view,
        left_bar_button_item,
        right_bar_button_item,
        back_bar_button_item,
        hides_back_button: _,
    } = env.objc.borrow(this);
    release(env, title);
    release(env, title_view);
    release(env, left_bar_button_item);
    release(env, right_bar_button_item);
    release(env, back_bar_button_item);
    env.objc.dealloc_object(this, &mut env.mem);
}

- (id)title {
    env.objc.borrow::<UINavigationItemHostObject>(this).title
}
- (())setTitle:(id)title { // NSString*
    let new: id = if title == nil { nil } else { msg![env; title copy] };
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).title, new);
    release(env, old);
}

- (id)titleView {
    env.objc.borrow::<UINavigationItemHostObject>(this).title_view
}
- (())setTitleView:(id)view { // UIView*
    retain(env, view);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).title_view, view);
    release(env, old);
}

- (id)leftBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).left_bar_button_item
}
- (())setLeftBarButtonItem:(id)item { // UIBarButtonItem*
    retain(env, item);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).left_bar_button_item, item);
    release(env, old);
}
- (())setLeftBarButtonItem:(id)item animated:(bool)_animated {
    () = msg![env; this setLeftBarButtonItem:item];
}

- (id)rightBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).right_bar_button_item
}
- (())setRightBarButtonItem:(id)item { // UIBarButtonItem*
    retain(env, item);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).right_bar_button_item, item);
    release(env, old);
}
- (())setRightBarButtonItem:(id)item animated:(bool)_animated {
    () = msg![env; this setRightBarButtonItem:item];
}

- (id)backBarButtonItem {
    env.objc.borrow::<UINavigationItemHostObject>(this).back_bar_button_item
}
- (())setBackBarButtonItem:(id)item { // UIBarButtonItem*
    retain(env, item);
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UINavigationItemHostObject>(this).back_bar_button_item, item);
    release(env, old);
}

- (bool)hidesBackButton {
    env.objc.borrow::<UINavigationItemHostObject>(this).hides_back_button
}
- (())setHidesBackButton:(bool)hides {
    env.objc.borrow_mut::<UINavigationItemHostObject>(this).hides_back_button = hides;
}
- (())setHidesBackButton:(bool)hides animated:(bool)_animated {
    () = msg![env; this setHidesBackButton:hides];
}

@end

};
