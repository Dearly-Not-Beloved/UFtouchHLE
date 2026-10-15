/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSCalendar` and `NSDateComponents`.
//!
//! Only the Gregorian calendar is supported, and like the rest of touchHLE's
//! date handling, everything is in GMT (see `NSTimeZone`).

use super::{ns_string, NSInteger, NSTimeInterval, NSUInteger};
use crate::dyld::{ConstantExports, HostConstant};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};

pub const NSGregorianCalendar: &str = "gregorian";

pub const CONSTANTS: ConstantExports = &[(
    "_NSGregorianCalendar",
    HostConstant::NSString(NSGregorianCalendar),
)];

type NSCalendarUnit = NSUInteger;
const NSEraCalendarUnit: NSCalendarUnit = 1 << 1;
const NSYearCalendarUnit: NSCalendarUnit = 1 << 2;
const NSMonthCalendarUnit: NSCalendarUnit = 1 << 3;
const NSDayCalendarUnit: NSCalendarUnit = 1 << 4;
const NSHourCalendarUnit: NSCalendarUnit = 1 << 5;
const NSMinuteCalendarUnit: NSCalendarUnit = 1 << 6;
const NSSecondCalendarUnit: NSCalendarUnit = 1 << 7;
const NSWeekdayCalendarUnit: NSCalendarUnit = 1 << 9;

const NSUndefinedDateComponent: NSInteger = 0x7fffffff;

const SECS_PER_DAY: i64 = 24 * 60 * 60;

struct NSCalendarHostObject {
    /// `NSString*`
    identifier: id,
}
impl HostObject for NSCalendarHostObject {}

#[derive(Clone, Copy)]
struct NSDateComponentsHostObject {
    era: NSInteger,
    year: NSInteger,
    month: NSInteger,
    day: NSInteger,
    hour: NSInteger,
    minute: NSInteger,
    second: NSInteger,
    weekday: NSInteger,
}
impl HostObject for NSDateComponentsHostObject {}
impl Default for NSDateComponentsHostObject {
    fn default() -> Self {
        NSDateComponentsHostObject {
            era: NSUndefinedDateComponent,
            year: NSUndefinedDateComponent,
            month: NSUndefinedDateComponent,
            day: NSUndefinedDateComponent,
            hour: NSUndefinedDateComponent,
            minute: NSUndefinedDateComponent,
            second: NSUndefinedDateComponent,
            weekday: NSUndefinedDateComponent,
        }
    }
}

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar.
/// From Howard Hinnant's `days_from_civil` algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The inverse of [days_from_civil].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// Days from 1970-01-01 to the reference date (2001-01-01).
const REFERENCE_DATE_DAYS: i64 = 11323;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSCalendar: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSCalendarHostObject { identifier: nil });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)currentCalendar {
    let identifier = ns_string::get_static_str(env, NSGregorianCalendar);
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCalendarIdentifier:identifier];
    autorelease(env, new)
}

- (id)initWithCalendarIdentifier:(id)identifier { // NSString*
    let gregorian = ns_string::get_static_str(env, NSGregorianCalendar);
    if !msg![env; identifier isEqualToString:gregorian] {
        log!(
            "TODO: calendar {:?}, using the Gregorian calendar instead",
            ns_string::to_rust_string(env, identifier)
        );
    }
    let identifier: id = msg![env; identifier copy];
    env.objc.borrow_mut::<NSCalendarHostObject>(this).identifier = identifier;
    this
}

- (())dealloc {
    let identifier = env.objc.borrow::<NSCalendarHostObject>(this).identifier;
    release(env, identifier);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)calendarIdentifier {
    env.objc.borrow::<NSCalendarHostObject>(this).identifier
}

- (id)components:(NSCalendarUnit)units
        fromDate:(id)date { // NSDate*
    let time: NSTimeInterval = msg![env; date timeIntervalSinceReferenceDate];
    let secs = time.floor() as i64;
    let days = secs.div_euclid(SECS_PER_DAY) + REFERENCE_DATE_DAYS;
    let secs_of_day = secs.rem_euclid(SECS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 was a Thursday. Sunday is 1.
    let weekday = (days + 4).rem_euclid(7) + 1;

    let mut c = NSDateComponentsHostObject::default();
    let wanted = |unit: NSCalendarUnit| units & unit != 0;
    if wanted(NSEraCalendarUnit) {
        c.era = if year > 0 { 1 } else { 0 };
    }
    if wanted(NSYearCalendarUnit) {
        c.year = year as NSInteger;
    }
    if wanted(NSMonthCalendarUnit) {
        c.month = month as NSInteger;
    }
    if wanted(NSDayCalendarUnit) {
        c.day = day as NSInteger;
    }
    if wanted(NSHourCalendarUnit) {
        c.hour = (secs_of_day / 3600) as NSInteger;
    }
    if wanted(NSMinuteCalendarUnit) {
        c.minute = (secs_of_day / 60 % 60) as NSInteger;
    }
    if wanted(NSSecondCalendarUnit) {
        c.second = (secs_of_day % 60) as NSInteger;
    }
    if wanted(NSWeekdayCalendarUnit) {
        c.weekday = weekday as NSInteger;
    }

    let components: id = msg_class![env; NSDateComponents new];
    *env.objc.borrow_mut::<NSDateComponentsHostObject>(components) = c;
    autorelease(env, components)
}

- (id)dateFromComponents:(id)components { // NSDateComponents*
    let c = *env.objc.borrow::<NSDateComponentsHostObject>(components);
    let or = |value: NSInteger, default: i64| {
        if value == NSUndefinedDateComponent { default } else { value as i64 }
    };
    let days = days_from_civil(or(c.year, 1), or(c.month, 1), or(c.day, 1));
    let secs = (days - REFERENCE_DATE_DAYS) * SECS_PER_DAY
        + or(c.hour, 0) * 3600
        + or(c.minute, 0) * 60
        + or(c.second, 0);
    msg_class![env; NSDate dateWithTimeIntervalSinceReferenceDate:(secs as NSTimeInterval)]
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

@end

@implementation NSDateComponents: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<NSDateComponentsHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (NSInteger)era { env.objc.borrow::<NSDateComponentsHostObject>(this).era }
- (NSInteger)year { env.objc.borrow::<NSDateComponentsHostObject>(this).year }
- (NSInteger)month { env.objc.borrow::<NSDateComponentsHostObject>(this).month }
- (NSInteger)day { env.objc.borrow::<NSDateComponentsHostObject>(this).day }
- (NSInteger)hour { env.objc.borrow::<NSDateComponentsHostObject>(this).hour }
- (NSInteger)minute { env.objc.borrow::<NSDateComponentsHostObject>(this).minute }
- (NSInteger)second { env.objc.borrow::<NSDateComponentsHostObject>(this).second }
- (NSInteger)weekday { env.objc.borrow::<NSDateComponentsHostObject>(this).weekday }

- (())setEra:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).era = value;
}
- (())setYear:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).year = value;
}
- (())setMonth:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).month = value;
}
- (())setDay:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).day = value;
}
- (())setHour:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).hour = value;
}
- (())setMinute:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).minute = value;
}
- (())setSecond:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).second = value;
}
- (())setWeekday:(NSInteger)value {
    env.objc.borrow_mut::<NSDateComponentsHostObject>(this).weekday = value;
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    let c = *env.objc.borrow::<NSDateComponentsHostObject>(this);
    let new: id = msg_class![env; NSDateComponents new];
    *env.objc.borrow_mut::<NSDateComponentsHostObject>(new) = c;
    new
}

@end

};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2001, 1, 1), REFERENCE_DATE_DAYS);
        assert_eq!(civil_from_days(REFERENCE_DATE_DAYS), (2001, 1, 1));
        // A leap day
        let days = days_from_civil(2024, 2, 29);
        assert_eq!(civil_from_days(days), (2024, 2, 29));
        assert_eq!(civil_from_days(days + 1), (2024, 3, 1));
    }
}
