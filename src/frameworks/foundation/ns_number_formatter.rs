/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSNumberFormatter`.
//!
//! This is very basic: locales are ignored, and only the "no style",
//! "decimal" and "currency" styles are formatted differently.

use super::{ns_string, NSInteger};
use crate::objc::{
    autorelease, id, msg, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};

type NSNumberFormatterStyle = NSInteger;
const NSNumberFormatterNoStyle: NSNumberFormatterStyle = 0;
const NSNumberFormatterDecimalStyle: NSNumberFormatterStyle = 1;
const NSNumberFormatterCurrencyStyle: NSNumberFormatterStyle = 2;

struct NSNumberFormatterHostObject {
    style: NSNumberFormatterStyle,
    /// `NSLocale*`
    locale: id,
}
impl HostObject for NSNumberFormatterHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSNumberFormatter: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSNumberFormatterHostObject {
        style: NSNumberFormatterNoStyle,
        locale: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())dealloc {
    let locale = env.objc.borrow::<NSNumberFormatterHostObject>(this).locale;
    release(env, locale);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (())setFormatterBehavior:(NSInteger)_behavior {
    // Only the "10.4+" behavior exists on iPhone OS (see NSDateFormatter).
}

- (NSNumberFormatterStyle)numberStyle {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).style
}
- (())setNumberStyle:(NSNumberFormatterStyle)style {
    env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).style = style;
}

- (id)locale {
    env.objc.borrow::<NSNumberFormatterHostObject>(this).locale
}
- (())setLocale:(id)locale { // NSLocale*
    retain(env, locale);
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<NSNumberFormatterHostObject>(this).locale,
        locale,
    );
    release(env, old);
}

- (id)stringFromNumber:(id)number { // NSNumber*
    if number == nil {
        return nil;
    }
    if let Some(text) = crate::frameworks::store_kit::sk_product::unavailable_price_text(env, number) {
        return ns_string::get_static_str(env, text);
    }
    let style = env.objc.borrow::<NSNumberFormatterHostObject>(this).style;
    let string = match style {
        NSNumberFormatterCurrencyStyle => {
            let value: f64 = msg![env; number doubleValue];
            // TODO: other currencies
            let locale = env.objc.borrow::<NSNumberFormatterHostObject>(this).locale;
            let is_us = locale != nil && {
                let identifier: id = msg![env; locale localeIdentifier];
                ns_string::to_rust_string(env, identifier).contains("US")
            };
            format!("{}{:.2}", if is_us { "$" } else { "" }, value)
        }
        NSNumberFormatterDecimalStyle => {
            let value: f64 = msg![env; number doubleValue];
            // Up to 3 fraction digits, without trailing zeros.
            let s = format!("{:.3}", value);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        _ => {
            if style != NSNumberFormatterNoStyle {
                log!("TODO: NSNumberFormatter style {}", style);
            }
            let description: id = msg![env; number stringValue];
            ns_string::to_rust_string(env, description).to_string()
        }
    };
    let string = ns_string::from_rust_string(env, string);
    autorelease(env, string)
}

@end

};
