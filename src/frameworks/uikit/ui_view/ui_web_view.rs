/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UIWebView`.
//!
//! There's no web engine: pages aren't loaded or shown, and JavaScript isn't
//! run. Web views can still be created, configured and shown (blank), which
//! keeps in-app web screens such as social SDKs' from crashing the app.

use crate::frameworks::core_graphics::CGRect;
use crate::frameworks::foundation::ns_string::{get_static_str, to_rust_string};
use crate::frameworks::foundation::NSUInteger;
use crate::msg;
use crate::objc::{
    id, impl_HostObject_with_superclass, msg_class, msg_super, nil, objc_classes, release,
    ClassExports, NSZonePtr,
};
use std::borrow::Cow;

#[derive(Default)]
struct UIWebViewHostObject {
    superclass: super::UIViewHostObject,
    /// `UIScrollView*`, created on first use.
    scroll_view: id,
    /// Not retained.
    delegate: id,
}
impl_HostObject_with_superclass!(UIWebViewHostObject);

/// UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight
const FLEXIBLE_SIZE: NSUInteger = (1 << 1) | (1 << 4);

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIWebView: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UIWebViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// NSCoding implementation
- (id)initWithCoder:(id)_coder {
    todo!()
}

- (())dealloc {
    let scroll_view = env.objc.borrow::<UIWebViewHostObject>(this).scroll_view;
    release(env, scroll_view);
    msg_super![env; this dealloc]
}

// The scroll view showing the page: a subview filling the web view (iOS 5
// and later).
- (id)scrollView {
    let existing = env.objc.borrow::<UIWebViewHostObject>(this).scroll_view;
    if existing != nil {
        return existing;
    }
    let bounds: CGRect = msg![env; this bounds];
    let scroll_view: id = msg_class![env; UIScrollView alloc];
    let scroll_view: id = msg![env; scroll_view initWithFrame:bounds];
    () = msg![env; scroll_view setAutoresizingMask:FLEXIBLE_SIZE];
    () = msg![env; this addSubview:scroll_view];
    env.objc.borrow_mut::<UIWebViewHostObject>(this).scroll_view = scroll_view;
    scroll_view
}

- (())setScalesPageToFit:(bool)_scales {
    // TODO
}
- (id)delegate {
    env.objc.borrow::<UIWebViewHostObject>(this).delegate
}
- (())setDelegate:(id)delegate {
    env.objc.borrow_mut::<UIWebViewHostObject>(this).delegate = delegate;
}

- (())loadRequest:(id)request { // NSURLRequest*
    let url_string = if request != nil {
        let url = msg![env; request URL];
        let url_desc = msg![env; url description];
        to_rust_string(env, url_desc)
    } else {
        Cow::default()
    };
    log!("TODO: [(UIWebView*) {:?} loadRequest:{:?} ({})]", this, request, url_string);
}
- (())loadHTMLString:(id)html // NSString*
             baseURL:(id)base_url { // NSURL*
    let length: NSUInteger = if html == nil { 0 } else { msg![env; html length] };
    log!(
        "TODO: [(UIWebView*) {:?} loadHTMLString:({} characters) baseURL:{:?}]",
        this,
        length,
        base_url,
    );
}
// Nothing is ever loaded.
- (id)request {
    nil
}
- (bool)isLoading {
    false
}
- (())stopLoading {
}

// Without a JavaScript engine, every script gives an empty result, which is
// also what iOS returns for scripts without a string result.
- (id)stringByEvaluatingJavaScriptFromString:(id)script { // NSString*
    let script = if script == nil { Cow::default() } else { to_rust_string(env, script) };
    log!(
        "TODO: [(UIWebView*) {:?} stringByEvaluatingJavaScriptFromString:{:?}] (not run)",
        this,
        script,
    );
    get_static_str(env, "")
}

@end

};
