/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `SKPayment`, `SKMutablePayment` and `SKPaymentTransaction`.

use crate::frameworks::foundation::{ns_string, NSInteger};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};

pub type SKPaymentTransactionState = NSInteger;
pub const SKPaymentTransactionStateFailed: SKPaymentTransactionState = 2;

/// `SKErrorPaymentNotAllowed`: the device isn't allowed to make payments.
const SKErrorPaymentNotAllowed: NSInteger = 4;

struct SKPaymentHostObject {
    /// `NSString*`
    product_identifier: id,
    quantity: NSInteger,
}
impl HostObject for SKPaymentHostObject {}

struct SKPaymentTransactionHostObject {
    /// `SKPayment*`
    payment: id,
    state: SKPaymentTransactionState,
    /// `NSError*`
    error: id,
}
impl HostObject for SKPaymentTransactionHostObject {}

/// Make a transaction for `payment` that failed, as all payments do.
pub fn new_failed_transaction(env: &mut crate::Environment, payment: id) -> id {
    let domain = ns_string::get_static_str(env, "SKErrorDomain");
    let error: id = msg_class![env; NSError alloc];
    let error: id = msg![env; error initWithDomain:domain
                                             code:SKErrorPaymentNotAllowed
                                         userInfo:nil];
    retain(env, payment);
    let host_object = Box::new(SKPaymentTransactionHostObject {
        payment,
        state: SKPaymentTransactionStateFailed,
        error,
    });
    let class = env
        .objc
        .get_known_class("SKPaymentTransaction", &mut env.mem);
    env.objc.alloc_object(class, host_object, &mut env.mem)
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation SKPayment: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKPaymentHostObject {
        product_identifier: nil,
        quantity: 1,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)paymentWithProductIdentifier:(id)identifier { // NSString*
    let new: id = msg![env; this new];
    let identifier: id = msg![env; identifier copy];
    env.objc.borrow_mut::<SKPaymentHostObject>(new).product_identifier = identifier;
    autorelease(env, new)
}

+ (id)paymentWithProduct:(id)product { // SKProduct*
    let identifier: id = if product == nil {
        nil
    } else {
        msg![env; product productIdentifier]
    };
    msg![env; this paymentWithProductIdentifier:identifier]
}

- (())dealloc {
    let identifier = env.objc.borrow::<SKPaymentHostObject>(this).product_identifier;
    release(env, identifier);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)productIdentifier {
    env.objc.borrow::<SKPaymentHostObject>(this).product_identifier
}

- (NSInteger)quantity {
    env.objc.borrow::<SKPaymentHostObject>(this).quantity
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    let &SKPaymentHostObject {
        product_identifier,
        quantity,
    } = env.objc.borrow(this);
    let new: id = msg_class![env; SKPayment new];
    retain(env, product_identifier);
    *env.objc.borrow_mut::<SKPaymentHostObject>(new) = SKPaymentHostObject {
        product_identifier,
        quantity,
    };
    new
}

@end

@implementation SKMutablePayment: SKPayment

- (())setProductIdentifier:(id)identifier { // NSString*
    let identifier: id = msg![env; identifier copy];
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<SKPaymentHostObject>(this).product_identifier,
        identifier,
    );
    release(env, old);
}

- (())setQuantity:(NSInteger)quantity {
    env.objc.borrow_mut::<SKPaymentHostObject>(this).quantity = quantity;
}

@end

@implementation SKPaymentTransaction: NSObject

- (())dealloc {
    let &SKPaymentTransactionHostObject { payment, error, .. } = env.objc.borrow(this);
    release(env, payment);
    release(env, error);
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)payment {
    env.objc.borrow::<SKPaymentTransactionHostObject>(this).payment
}

- (SKPaymentTransactionState)transactionState {
    env.objc.borrow::<SKPaymentTransactionHostObject>(this).state
}

- (id)error {
    env.objc.borrow::<SKPaymentTransactionHostObject>(this).error
}

- (id)transactionIdentifier {
    nil
}

- (id)transactionReceipt {
    nil
}

- (id)transactionDate {
    nil
}

- (id)originalTransaction {
    nil
}

@end

};
