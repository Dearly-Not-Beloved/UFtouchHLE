/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSURLConnection`.

use super::{ns_string, NSInteger};
use crate::environment::Environment;
use crate::mem::MutPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use std::borrow::Cow;

const NSURLErrorDomain: &str = "NSURLErrorDomain";

/// Our helper type, Foundation just uses ints.
type NSURLErrorCode = NSInteger;
const NSURLErrorNotConnectedToInternet: NSURLErrorCode = -1009;

#[derive(Default)]
pub struct State {
    pending: Vec<id>,
}
#[derive(Default)]
struct Connection {
    request: id,
    delegate: id,
    started: bool,
    cancelled: bool,
}
impl HostObject for Connection {}

/// The local GameKit backend does not provide HTTP. Report a real asynchronous
/// failure on the main loop instead of returning nil and leaving SDK timers
/// holding a nonexistent connection. No HTTP request is made.
pub(crate) fn pump(env: &mut Environment) {
    let pending = std::mem::take(&mut env.framework_state.foundation.ns_url_connection.pending);
    for connection in pending {
        let c = env.objc.borrow::<Connection>(connection);
        let (delegate, cancelled) = (c.delegate, c.cancelled);
        if !cancelled && delegate != nil {
            let selector = env
                .objc
                .register_host_selector("connection:didFailWithError:".into(), &mut env.mem);
            let responds: bool = msg![env; delegate respondsToSelector:selector];
            if responds {
                let domain = ns_string::get_static_str(env, NSURLErrorDomain);
                let error: id = msg_class![env; NSError errorWithDomain:domain code:NSURLErrorNotConnectedToInternet userInfo:nil];
                retain(env, error);
                retain(env, delegate);
                let _: () = msg![env; delegate connection:connection didFailWithError:error];
                release(env, delegate);
                release(env, error);
            }
        }
        let c = env.objc.borrow_mut::<Connection>(connection);
        let delegate = std::mem::replace(&mut c.delegate, nil);
        release(env, delegate);
        release(env, connection);
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSURLConnection: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    env.objc.alloc_object(this,Box::<Connection>::default(),&mut env.mem)
}
+ (id)sendSynchronousRequest:(id)request // NSURLRequest *
           returningResponse:(MutPtr<id>)response // NSURLResponse **
                       error:(MutPtr<id>)out_error { // NSError **
    log!(
        "TODO: [NSURLConnection sendSynchronousRequest:{:?} ('{}') response:{:?} error:{:?}] -> nil",
        request,
        url_string_from_request(env, request),
        response,
        out_error,
    );
    if !response.is_null() {
        env.mem.write(response, nil);
    }
    if !out_error.is_null() {
        let domain = ns_string::get_static_str(env, NSURLErrorDomain);
        let error = msg_class![env; NSError alloc];
        // TODO: fill userInfo
        let error = msg![env; error initWithDomain:domain code:NSURLErrorNotConnectedToInternet userInfo:nil];
        autorelease(env, error);
        env.mem.write(out_error, error);
        if env.options.network_trace {
            crate::network_trace::record(env, "network_error", serde_json::json!({
                "api":"NSURLConnection.sendSynchronousRequest",
                "error_object":error.to_bits(), "domain":NSURLErrorDomain,
                "code":NSURLErrorNotConnectedToInternet, "source":"existing_stub"
            }));
        }
    }
    nil
}

+ (id)connectionWithRequest:(id)request // NSURLRequest *
                   delegate:(id)delegate {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithRequest:request delegate:delegate];
    autorelease(env, new)
}

- (())dealloc {
    let c=env.objc.borrow::<Connection>(this);
    let (request,delegate)=(c.request,c.delegate);
    release(env,request);release(env,delegate);
    env.objc.dealloc_object(this,&mut env.mem)
}
- (())start {
    let c=env.objc.borrow_mut::<Connection>(this);
    if c.started || c.cancelled {return;}
    c.started=true;
    retain(env,this);
    env.framework_state.foundation.ns_url_connection.pending.push(this);
}
- (())cancel {env.objc.borrow_mut::<Connection>(this).cancelled=true;}
- (())scheduleInRunLoop:(id)_run_loop forMode:(id)_mode {
    // This local-only failure backend is drained by the existing main loop.
}
- (())unscheduleFromRunLoop:(id)_run_loop forMode:(id)_mode {}
- (id)originalRequest {env.objc.borrow::<Connection>(this).request}
- (id)currentRequest {env.objc.borrow::<Connection>(this).request}

- (id)initWithRequest:(id)request // NSURLRequest *
             delegate:(id)delegate {
    msg![env; this initWithRequest:request delegate:delegate startImmediately:true]
}

- (id)initWithRequest:(id)request // NSURLRequest *
             delegate:(id)delegate
     startImmediately:(bool)start_immediately {
    if env.options.network_access && (env.options.gamekit_role.is_some() || env.options.gamekit_discover.is_some()) {
        retain(env,request);retain(env,delegate);
        let c=env.objc.borrow_mut::<Connection>(this);
        c.request=request;c.delegate=delegate;
        if start_immediately {let _:()=msg![env; this start];}
        return this;
    }
    log!(
        "TODO: [(NSURLConnection *){:?} initWithRequest:{:?} ('{}') delegate:{:?} startImmediately:{}] -> nil",
        this,
        request,
        url_string_from_request(env, request),
        delegate,
        start_immediately,
    );
    release(env, this);
    nil
}

@end

};

fn url_string_from_request(env: &mut Environment, request: id) -> Cow<'static, str> {
    if request == nil {
        Cow::from("(null)")
    } else {
        let url = msg![env; request URL];
        let ns_string = msg![env; url absoluteString];
        ns_string::to_rust_string(env, ns_string)
    }
}
