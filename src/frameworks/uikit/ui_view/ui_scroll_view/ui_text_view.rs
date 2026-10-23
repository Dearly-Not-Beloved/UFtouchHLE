/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UITextView`.

use crate::frameworks::core_graphics::cg_context::CGContextSetRGBFillColor;
use crate::frameworks::core_graphics::cg_geometry::CGPointZero;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::{self, to_rust_string};
use crate::frameworks::foundation::{NSInteger, NSRange, NSUInteger};
use crate::frameworks::uikit::ui_color;
use crate::frameworks::uikit::ui_font::{
    break_lines_with_font, UILineBreakModeTailTruncation, UILineBreakModeWordWrap, UITextAlignment,
    UITextAlignmentLeft,
};
use crate::frameworks::uikit::ui_graphics::UIGraphicsGetCurrentContext;
use crate::frameworks::uikit::ui_view::ui_control::ui_text_field::UIReturnKeyType;
use crate::frameworks::uikit::ui_view::ui_window::{
    UIKeyboardDidHideNotification, UIKeyboardDidShowNotification, UIKeyboardWillHideNotification,
    UIKeyboardWillShowNotification,
};
use crate::objc::{
    autorelease, id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes,
    release, retain, todo_objc_setter, ClassExports, NSZonePtr,
};
use crate::Environment;

type UIDataDetectorTypes = NSUInteger;

pub struct UITextViewHostObject {
    superclass: super::UIScrollViewHostObject,
    editable: bool,
    /// `NSString*`
    text: id,
    /// `UIFont*`
    font: id,
    /// `UIColor*`
    text_color: id,
    text_alignment: UITextAlignment,
    /// Weak reference to the `UITextViewDelegate`.
    delegate: id,
    /// Whether it's the first responder and receiving keyboard input.
    editing: bool,
}
impl_HostObject_with_superclass!(UITextViewHostObject);
impl Default for UITextViewHostObject {
    fn default() -> Self {
        UITextViewHostObject {
            superclass: Default::default(),
            // Text views are editable by default on iPhone OS.
            editable: true,
            font: nil,
            text: nil,
            text_color: nil,
            text_alignment: UITextAlignmentLeft,
            delegate: nil,
            editing: false,
        }
    }
}

// Update contentOffset and contentSize when anything that potentially affects
// contentSize like font and text change.
fn update_scroll(env: &mut Environment, this: id) {
    let bounds: CGRect = msg![env; this bounds];
    let bound_size = bounds.size;
    let font: id = msg![env; this font];
    let text: id = msg![env; this text];

    // Calculate our new contentSize
    let calculated_size: CGSize = msg![env; text sizeWithFont:font constrainedToSize:bound_size];
    () = msg![env; this setContentSize:calculated_size];

    // Reset contentOffset if we have now gone out of bounds of contentSize,
    // otherwise ignore.
    let current_content_offset: CGPoint = msg![env; this contentOffset];
    if current_content_offset.x > calculated_size.width - bounds.size.width
        || current_content_offset.y > calculated_size.height - bounds.size.height
    {
        () = msg![env; this setContentOffset:(CGPoint { x: 0.0, y: 0.0 })];
    }
}
fn delegate_responds(env: &mut Environment, delegate: id, selector: &str) -> bool {
    delegate != nil
        && env
            .objc
            .object_has_method_named(&env.mem, delegate, selector)
}

/// Replace the characters in `range` with `replacement` as if the user typed
/// it, if the delegate allows it.
fn replace_text(
    env: &mut Environment,
    text_view: id,
    location: NSUInteger,
    length: NSUInteger,
    replacement: id,
) {
    let delegate = env.objc.borrow::<UITextViewHostObject>(text_view).delegate;
    if delegate_responds(
        env,
        delegate,
        "textView:shouldChangeTextInRange:replacementText:",
    ) && !msg![env; delegate textView:text_view
                         shouldChangeTextInRange:(NSRange { location, length })
                                 replacementText:replacement]
    {
        return;
    }
    let text: id = msg![env; text_view text];
    let before: id = msg![env; text substringToIndex:location];
    let after: id = msg![env; text substringFromIndex:(location + length)];
    let new_text: id = msg![env; before stringByAppendingString:replacement];
    let new_text: id = msg![env; new_text stringByAppendingString:after];
    () = msg![env; text_view setText:new_text];
    if delegate_responds(env, delegate, "textViewDidChange:") {
        () = msg![env; delegate textViewDidChange:text_view];
    }
}

/// Keyboard input for the text view that's the first responder.
pub fn handle_text(env: &mut Environment, text_view: id, text: String) {
    let current: id = msg![env; text_view text];
    let length: NSUInteger = msg![env; current length];
    let replacement = ns_string::from_rust_string(env, text);
    replace_text(env, text_view, length, 0, replacement);
    release(env, replacement);
}
pub fn handle_backspace(env: &mut Environment, text_view: id) {
    let current: id = msg![env; text_view text];
    let length: NSUInteger = msg![env; current length];
    if length == 0 {
        return;
    }
    let empty = ns_string::get_static_str(env, "");
    replace_text(env, text_view, length - 1, 1, empty);
}
/// The return key types a newline (apps' delegates often check for it to end
/// editing).
pub fn handle_return(env: &mut Environment, text_view: id) {
    handle_text(env, text_view, "\n".to_string());
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UITextView: UIScrollView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UITextViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    let this: id = msg_super![env; this initWithFrame:frame];
    // TODO: refactor to a common init with `initWithCoder:`
    // These aren't redundant, the setters fetch the real defaults.
    () = msg![env; this setFont:nil];
    () = msg![env; this setTextColor:nil];
    // Built-in views don't have user-controlled opaqueness. (Otherwise, with
    // a clear background color, the text is drawn on black.)
    () = msg_super![env; this setOpaque:false];
    this
}

- (id)initWithCoder:(id)coder {
    let this: id = msg_super![env; this initWithCoder:coder];
    // These aren't redundant, the setters fetch the real defaults.
    () = msg![env; this setFont:nil];
    () = msg![env; this setTextColor:nil];
    // Built-in views don't have user-controlled opaqueness.
    () = msg_super![env; this setOpaque:false];
    let key = ns_string::get_static_str(env, "UIEditable");
    if msg![env; coder containsValueForKey:key] {
        let editable: bool = msg![env; coder decodeBoolForKey:key];
        env.objc.borrow_mut::<UITextViewHostObject>(this).editable = editable;
    }
    this
}

- (())dealloc {
    let UITextViewHostObject {
        superclass: _,
        editable: _,
        font,
        text,
        text_color,
        text_alignment: _,
        delegate: _,
        editing,
    } = std::mem::take(env.objc.borrow_mut(this));
    if editing && env.framework_state.uikit.ui_responder.first_responder == this {
        env.framework_state.uikit.ui_responder.first_responder = nil;
        env.on_parent_stack_in_coroutine(|window, _| window.stop_text_input());
    }

    release(env, font);
    release(env, text_color);
    release(env, text);
    msg_super![env; this dealloc]
}

- (id)text {
    env.objc.borrow::<UITextViewHostObject>(this).text
}
- (())setText:(id)new_text { // NSString*
    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_text = std::mem::replace(&mut hostobj.text, new_text);
    retain(env, new_text);
    release(env, old_text);
    update_scroll(env,this);
    () = msg![env; this setNeedsDisplay];
}

// Attributes aren't supported, only the text is used.
- (id)attributedText {
    let text: id = msg![env; this text];
    let string: id = msg_class![env; NSAttributedString alloc];
    let string: id = msg![env; string initWithString:text];
    autorelease(env, string)
}
- (())setAttributedText:(id)attributed_text { // NSAttributedString*
    let text: id = msg![env; attributed_text string];
    () = msg![env; this setText:text];
}

// weak/non-retaining
- (())setDelegate:(id)delegate { // UITextViewDelegate
    env.objc.borrow_mut::<UITextViewHostObject>(this).delegate = delegate;
}
- (id)delegate {
    env.objc.borrow::<UITextViewHostObject>(this).delegate
}

- (())setOpaque:(bool)_opaque {
    // Built-in views don't have user-controlled opaqueness.
}

// Text views handle touches themselves, rather than passing them on to the
// next responder like other views: a tap starts editing.
- (())touchesBegan:(id)_touches // NSSet* of UITouch*
         withEvent:(id)_event { // UIEvent*
    let _: bool = msg![env; this becomeFirstResponder];
}
- (())touchesEnded:(id)_touches // NSSet* of UITouch*
         withEvent:(id)_event { // UIEvent*
}
- (())touchesCancelled:(id)_touches // NSSet* of UITouch*
             withEvent:(id)_event { // UIEvent*
}

- (bool)isFirstResponder {
    env.objc.borrow::<UITextViewHostObject>(this).editing
}

- (bool)becomeFirstResponder {
    let &UITextViewHostObject { editable, editing, delegate, .. } = env.objc.borrow(this);
    if editing {
        return true;
    }
    if !editable {
        return false;
    }
    if delegate_responds(env, delegate, "textViewShouldBeginEditing:")
        && !msg![env; delegate textViewShouldBeginEditing:this]
    {
        return false;
    }
    let text: id = msg![env; this text];
    if text == nil {
        let empty = ns_string::get_static_str(env, "");
        () = msg![env; this setText:empty];
    }

    let old_responder = env.framework_state.uikit.ui_responder.first_responder;
    if old_responder != nil && old_responder != this {
        let _: bool = msg![env; old_responder resignFirstResponder];
    }

    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    let name = ns_string::get_static_str(env, UIKeyboardWillShowNotification);
    () = msg![env; center postNotificationName:name object:this userInfo:nil];
    env.framework_state.uikit.ui_responder.first_responder = this;
    env.objc.borrow_mut::<UITextViewHostObject>(this).editing = true;
    env.on_parent_stack_in_coroutine(|window, _| window.start_text_input());
    let name = ns_string::get_static_str(env, UIKeyboardDidShowNotification);
    () = msg![env; center postNotificationName:name object:this userInfo:nil];

    if delegate_responds(env, delegate, "textViewDidBeginEditing:") {
        () = msg![env; delegate textViewDidBeginEditing:this];
    }
    true
}

- (bool)resignFirstResponder {
    let &UITextViewHostObject { editing, delegate, .. } = env.objc.borrow(this);
    if !editing {
        return true;
    }
    if delegate_responds(env, delegate, "textViewShouldEndEditing:")
        && !msg![env; delegate textViewShouldEndEditing:this]
    {
        return false;
    }

    let center: id = msg_class![env; NSNotificationCenter defaultCenter];
    let name = ns_string::get_static_str(env, UIKeyboardWillHideNotification);
    () = msg![env; center postNotificationName:name object:this userInfo:nil];
    if env.framework_state.uikit.ui_responder.first_responder == this {
        env.framework_state.uikit.ui_responder.first_responder = nil;
    }
    env.objc.borrow_mut::<UITextViewHostObject>(this).editing = false;
    env.on_parent_stack_in_coroutine(|window, _| window.stop_text_input());
    let name = ns_string::get_static_str(env, UIKeyboardDidHideNotification);
    () = msg![env; center postNotificationName:name object:this userInfo:nil];

    if delegate_responds(env, delegate, "textViewDidEndEditing:") {
        () = msg![env; delegate textViewDidEndEditing:this];
    }
    true
}

- (id)textColor {
    env.objc.borrow::<UITextViewHostObject>(this).text_color
}
- (())setTextColor:(id)new_text_color { // UIColor*
    let new_text_color: id = if new_text_color == nil {
        msg_class![env; UIColor blackColor]
    } else {
        new_text_color
    };

    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_text_color = std::mem::replace(&mut hostobj.text_color, new_text_color);
    retain(env, new_text_color);
    release(env, old_text_color);
    () = msg![env; this setNeedsDisplay];
}

- (UITextAlignment)textAlignment {
    env.objc.borrow::<UITextViewHostObject>(this).text_alignment
}
- (())setTextAlignment:(UITextAlignment)new_text_alignment {
    env.objc.borrow_mut::<UITextViewHostObject>(this).text_alignment = new_text_alignment;
    () = msg![env; this setNeedsDisplay];
}

- (id)font {
    env.objc.borrow::<UITextViewHostObject>(this).font
}
- (())setFont:(id)new_font { // UIFont*
    let new_font: id = if new_font == nil {
        // reset to default
        let size: CGFloat = 17.0;
        msg_class![env; UIFont systemFontOfSize:size]
    } else {
        new_font
    };

    let hostobj  = env.objc.borrow_mut::<UITextViewHostObject>(this);
    let old_font = std::mem::replace(&mut hostobj.font, new_font);
    retain(env, new_font);
    release(env, old_font);
    update_scroll(env,this);
    () = msg![env; this setNeedsDisplay];
}

- (())flashScrollIndicators {
    // TODO
}

// TODO: Make editable actually do something
- (bool)isEditable {
    env.objc.borrow::<UITextViewHostObject>(this).editable
}
- (())setEditable:(bool)editable {
    env.objc.borrow_mut::<UITextViewHostObject>(this).editable = editable;
}

- (())scrollRangeToVisible:(NSRange)range {
    let &mut UITextViewHostObject {
        font,
        text,
        ..
    } = env.objc.borrow_mut(this);

    if range.location > msg![env; text length] {
        return;
    }

    let bounds: CGRect = msg![env; this bounds];
    let bound_size = bounds.size;

    let text = to_rust_string(env, text);

    let lines = break_lines_with_font(env, font, &text, Some((bound_size, UILineBreakModeWordWrap)));

    let mut line_count = 0;
    let mut current_position = 0;

    for (_, line) in lines {
        current_position += line.len();

        if let Some(offset) = text[current_position..].find(|c: char| !c.is_whitespace()) {
            current_position += offset;
        } else {
            current_position = text.len();
        }

        if range.location <= current_position as u32 {
            break;
        }

        line_count += 1;
    }

    let line_height: CGFloat = msg![env; font lineHeight];
    let leading: CGFloat = msg![env; font leading];

    let height_to_range_start = (line_count + 1) as f32 * line_height - leading;

    let content_offset: CGPoint = msg![env; this contentOffset];

    if height_to_range_start - line_height < content_offset.y {
        let new_scroll_y = CGPoint {x: 0.0, y: line_count as f32 * line_height};
        () = msg![env; this setContentOffset:new_scroll_y];
    }
    else if height_to_range_start > content_offset.y + bound_size.height {
        let new_scroll_y = CGPoint {x: 0.0, y: height_to_range_start- bound_size.height};
        () = msg![env; this setContentOffset:new_scroll_y];
    }

    update_scroll(env, this);
}

- (())setReturnKeyType:(UIReturnKeyType)type_ {
    todo_objc_setter!(this, type_);
}

// Rest of the UITextInputTraits implementation
- (())setKeyboardType:(NSInteger)type_ {
    todo_objc_setter!(this, type_);
}
- (())setKeyboardAppearance:(NSInteger)appearance {
    todo_objc_setter!(this, appearance);
}
- (())setAutocapitalizationType:(NSInteger)type_ {
    todo_objc_setter!(this, type_);
}
- (())setAutocorrectionType:(NSInteger)type_ {
    todo_objc_setter!(this, type_);
}
- (())setSpellCheckingType:(NSInteger)type_ {
    todo_objc_setter!(this, type_);
}
- (())setEnablesReturnKeyAutomatically:(bool)enables {
    todo_objc_setter!(this, enables);
}
- (())setSecureTextEntry:(bool)secure {
    todo_objc_setter!(this, secure);
}
- (bool)isSecureTextEntry {
    false
}

- (())setDataDetectorTypes:(UIDataDetectorTypes)types {
    todo_objc_setter!(this, types);
}

- (())drawRect:(CGRect)_rect {
    let bounds: CGRect = msg![env; this bounds];
    let context = UIGraphicsGetCurrentContext(env);

    let &mut UITextViewHostObject {
        superclass: _,
        editable: _,
        font,
        text,
        text_color,
        text_alignment,
        ..
    } = env.objc.borrow_mut(this);

    let (r, g, b, a) = ui_color::get_rgba(&env.objc, text_color);
    CGContextSetRGBFillColor(env, context, r, g, b, a);

    let content_offset: CGPoint = msg![env; this contentOffset];
    let rect = CGRect {
        origin: CGPointZero,
        // If size is not expanded by the offset,
        // the text is rendered truncated.
        size: CGSize {
            width: bounds.size.width + content_offset.x,
            height: bounds.size.height + content_offset.y,
        }
    };

    log_dbg!("UItextView text rendering in rect {:?}", rect);
    let _size: CGSize = msg![env; text drawInRect:rect
                                         withFont:font
                                    lineBreakMode:UILineBreakModeTailTruncation
                                        alignment:text_alignment];
}

@end

};
