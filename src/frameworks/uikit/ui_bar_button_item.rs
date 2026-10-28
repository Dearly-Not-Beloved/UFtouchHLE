/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIBarItem` and `UIBarButtonItem`.
//!
//! Items only store their configuration. Bars don't draw them yet, except
//! that [UIToolbar](super::ui_view::ui_toolbar) shows items' custom views.

use crate::frameworks::core_graphics::CGFloat;
use crate::frameworks::foundation::NSInteger;
use crate::objc::{
    id, msg, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr, SEL,
};

type UIBarButtonItemStyle = NSInteger;
type UIBarButtonSystemItem = NSInteger;

struct UIBarButtonItemHostObject {
    /// `UIView*`
    custom_view: id,
    /// `NSString*`, copied
    title: id,
    /// `UIImage*`
    image: id,
    /// Not retained.
    target: id,
    action: Option<SEL>,
    style: UIBarButtonItemStyle,
    system_item: Option<UIBarButtonSystemItem>,
    width: CGFloat,
    enabled: bool,
    tag: NSInteger,
}
impl HostObject for UIBarButtonItemHostObject {}
impl Default for UIBarButtonItemHostObject {
    fn default() -> Self {
        UIBarButtonItemHostObject {
            custom_view: nil,
            title: nil,
            image: nil,
            target: nil,
            action: None,
            style: 0,
            system_item: None,
            width: 0.0,
            enabled: true,
            tag: 0,
        }
    }
}

fn set_retained(
    env: &mut crate::Environment,
    this: id,
    value: id,
    field: fn(&mut UIBarButtonItemHostObject) -> &mut id,
) {
    retain(env, value);
    let old = std::mem::replace(field(env.objc.borrow_mut(this)), value);
    release(env, old);
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIBarItem: NSObject
@end

@implementation UIBarButtonItem: UIBarItem

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIBarButtonItemHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithCustomView:(id)view { // UIView*
    () = msg![env; this setCustomView:view];
    this
}

- (id)initWithTitle:(id)title // NSString*
              style:(UIBarButtonItemStyle)style
             target:(id)target
             action:(SEL)action {
    () = msg![env; this setTitle:title];
    () = msg![env; this setStyle:style];
    () = msg![env; this setTarget:target];
    () = msg![env; this setAction:action];
    this
}

- (id)initWithImage:(id)image // UIImage*
              style:(UIBarButtonItemStyle)style
             target:(id)target
             action:(SEL)action {
    () = msg![env; this setImage:image];
    () = msg![env; this setStyle:style];
    () = msg![env; this setTarget:target];
    () = msg![env; this setAction:action];
    this
}

- (id)initWithBarButtonSystemItem:(UIBarButtonSystemItem)system_item
                           target:(id)target
                           action:(SEL)action {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).system_item = Some(system_item);
    () = msg![env; this setTarget:target];
    () = msg![env; this setAction:action];
    this
}

- (())dealloc {
    let &UIBarButtonItemHostObject { custom_view, title, image, .. } = env.objc.borrow(this);
    release(env, custom_view);
    release(env, title);
    release(env, image);
    env.objc.dealloc_object(this, &mut env.mem);
}

- (id)customView {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).custom_view
}
- (())setCustomView:(id)view { // UIView*
    set_retained(env, this, view, |h| &mut h.custom_view);
}

- (id)title {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).title
}
- (())setTitle:(id)title { // NSString*
    let copy: id = if title == nil { nil } else { msg![env; title copy] };
    let old = std::mem::replace(&mut env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).title, copy);
    release(env, old);
}

- (id)image {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).image
}
- (())setImage:(id)image { // UIImage*
    set_retained(env, this, image, |h| &mut h.image);
}

- (id)target {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).target
}
- (())setTarget:(id)target {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).target = target;
}

- (SEL)action {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).action.unwrap_or(SEL::null())
}
- (())setAction:(SEL)action {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).action = (!action.is_null()).then_some(action);
}

- (UIBarButtonItemStyle)style {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).style
}
- (())setStyle:(UIBarButtonItemStyle)style {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).style = style;
}

- (CGFloat)width {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).width
}
- (())setWidth:(CGFloat)width {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).width = width;
}

- (bool)isEnabled {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).enabled
}
- (())setEnabled:(bool)enabled {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).enabled = enabled;
}

- (NSInteger)tag {
    env.objc.borrow::<UIBarButtonItemHostObject>(this).tag
}
- (())setTag:(NSInteger)tag {
    env.objc.borrow_mut::<UIBarButtonItemHostObject>(this).tag = tag;
}

- (())setTintColor:(id)_color { // UIColor*
    // Items aren't drawn yet.
}

@end

};
