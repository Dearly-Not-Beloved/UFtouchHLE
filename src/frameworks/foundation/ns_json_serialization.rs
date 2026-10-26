/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! JSON writing for Foundation collections (used by P2G's iOS 7 startup).

use super::{ns_string, ns_value::NSNumberHostObject, NSInteger, NSUInteger};
use crate::mem::MutPtr;
use crate::objc::{id, msg, msg_class, nil, objc_classes, ClassExports};
use crate::Environment;
use serde_json::{Number, Value};

fn is_kind(env: &mut Environment, object: id, name: &str) -> bool {
    let class = env.objc.get_known_class(name, &mut env.mem);
    msg![env; object isKindOfClass:class]
}

fn value(env: &mut Environment, object: id, ancestors: &mut Vec<id>) -> Option<Value> {
    if object == nil || ancestors.len() >= 128 || ancestors.contains(&object) {
        return None;
    }
    if is_kind(env, object, "NSNull") {
        return Some(Value::Null);
    }
    if is_kind(env, object, "NSString") {
        return Some(Value::String(
            ns_string::to_rust_string(env, object).into_owned(),
        ));
    }
    if is_kind(env, object, "NSNumber") {
        let number = env.objc.borrow::<NSNumberHostObject>(object);
        return Some(match number {
            NSNumberHostObject::Bool(b) => Value::Bool(*b),
            NSNumberHostObject::UnsignedLongLong(n) => Value::Number((*n).into()),
            NSNumberHostObject::UnsignedInt(n) => Value::Number((*n).into()),
            NSNumberHostObject::UnsignedShort(n) => Value::Number((*n).into()),
            NSNumberHostObject::Float(n) => Value::Number(Number::from_f64(*n as f64)?),
            NSNumberHostObject::Double(n) => Value::Number(Number::from_f64(*n)?),
            _ => Value::Number(number.as_long_long().into()),
        });
    }
    ancestors.push(object);
    let result = if is_kind(env, object, "NSArray") {
        let count: NSUInteger = msg![env; object count];
        (0..count)
            .map(|i| {
                let child: id = msg![env; object objectAtIndex:i];
                value(env, child, ancestors)
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array)
    } else if is_kind(env, object, "NSDictionary") {
        let keys: id = msg![env; object keyEnumerator];
        let mut pairs = serde_json::Map::new();
        let mut valid = true;
        loop {
            let key: id = msg![env; keys nextObject];
            if key == nil {
                break;
            }
            if !is_kind(env, key, "NSString") {
                valid = false;
                break;
            }
            let child: id = msg![env; object objectForKey:key];
            let Some(child) = value(env, child, ancestors) else {
                valid = false;
                break;
            };
            pairs.insert(ns_string::to_rust_string(env, key).into_owned(), child);
        }
        valid.then_some(Value::Object(pairs))
    } else {
        None
    };
    ancestors.pop();
    result
}

fn document(env: &mut Environment, object: id) -> Option<Value> {
    let value = value(env, object, &mut Vec::new())?;
    matches!(value, Value::Array(_) | Value::Object(_)).then_some(value)
}

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation NSJSONSerialization: NSObject
+ (bool)isValidJSONObject:(id)object {
    document(env, object).is_some()
}

+ (id)dataWithJSONObject:(id)object options:(NSUInteger)options error:(MutPtr<id>)error {
    let bytes = document(env, object).and_then(|value| {
        if options & 1 != 0 {
            serde_json::to_vec_pretty(&value).ok()
        } else {
            serde_json::to_vec(&value).ok()
        }
    });
    let Some(bytes) = bytes else {
        if !error.is_null() {
            let domain = ns_string::get_static_str(env, "NSCocoaErrorDomain");
            let code: NSInteger = 3840;
            let failure: id = msg_class![env; NSError errorWithDomain:domain code:code userInfo:nil];
            env.mem.write(error, failure);
        }
        return nil;
    };
    let length: NSUInteger = bytes.len().try_into().unwrap();
    let buffer = env.mem.alloc(length);
    env.mem.bytes_at_mut(buffer.cast(), length).copy_from_slice(&bytes);
    msg_class![env; NSData dataWithBytesNoCopy:buffer length:length]
}
@end
};
