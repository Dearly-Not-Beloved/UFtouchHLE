/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSURL`.

use super::ns_string::{from_rust_string, get_static_str, to_rust_string};
use super::NSUInteger;
use crate::fs::{GuestPath, GuestPathBuf};
use crate::mem::MutPtr;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;
use std::borrow::Cow;

/// It seems like there's two kinds of NSURLs: ones for file paths, and others.
/// So far only the former is implemented (TODO).
enum NSURLHostObject {
    /// This is a file URL. The NSString is a system path (no `file:///`).
    ///
    /// This is a wrapper around NSString so that conversions between NSURL
    /// and NSString, which happen often, can be simple and efficient.
    FileURL {
        ns_string: id,
        // Relative file URL save the working directory at the time of creation
        // At the moment, used in the description selector.
        working_directory: GuestPathBuf,
    },
    /// Non-file URL.
    OtherURL { ns_string: id },
}
impl HostObject for NSURLHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSURL: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = NSURLHostObject::FileURL { ns_string: nil, working_directory: env.fs.working_directory().into() };
    env.objc.alloc_object(this, Box::new(host_object), &mut env.mem)
}

+ (id)URLWithString:(id)url { // NSString*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithString:url];
    autorelease(env, new)
}

+ (id)fileURLWithPath:(id)path { // NSString*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initFileURLWithPath:path];
    autorelease(env, new)
}

+ (id)fileURLWithPath:(id)path // NSString*
          isDirectory:(bool)is_dir {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initFileURLWithPath:path isDirectory:is_dir];
    autorelease(env, new)
}

- (())dealloc {
    match *env.objc.borrow(this) {
        NSURLHostObject::FileURL { ns_string, .. } => release(env, ns_string),
        NSURLHostObject::OtherURL { ns_string } => release(env, ns_string),
    }
    env.objc.dealloc_object(this, &mut env.mem)
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (id)initFileURLWithPath:(id)path { // NSString*
    // FIXME: this should guess whether the path is a directory
    msg![env; this initFileURLWithPath:path isDirectory:false]
}

- (id)initFileURLWithPath:(id)path // NSString*
              isDirectory:(bool)_is_dir {
    // FIXME: this does not resolve relative paths to be absolute!
    // TODO: this does not strip the file:/// prefix!
    assert!(!to_rust_string(env, path).starts_with("file:"));
    let path = msg![env; path stringByExpandingTildeInPath];
    let path: id = msg![env; path copy];
    *env.objc.borrow_mut(this) = NSURLHostObject::FileURL { ns_string: path, working_directory: env.fs.working_directory().into() };
    this
}

- (id)initWithString:(id)url { // NSString*
    if url == nil {
        return nil;
    }

    let url_str = to_rust_string(env, url);
    let parts = split_url(&url_str);
    if parts.scheme.is_some_and(|scheme| scheme.eq_ignore_ascii_case("file")) {
        let path = percent_decode(parts.path);
        let path = from_rust_string(env, path);
        let this: id = msg![env; this initFileURLWithPath:path];
        release(env, path);
        return this;
    }
    let url: id = msg![env; url copy];
    *env.objc.borrow_mut(this) = NSURLHostObject::OtherURL { ns_string: url };
    this
}

- (bool)isFileURL {
    match env.objc.borrow(this) {
        NSURLHostObject::FileURL { .. } => true,
        NSURLHostObject::OtherURL { .. } => false,
    }
}

- (bool)checkResourceIsReachableAndReturnError:(MutPtr<id>)error { // NSError**
    let is_file_url: bool = msg![env; this isFileURL];
    let reachable = is_file_url && {
        let path = to_rust_path(env, this);
        env.fs.exists(&path)
    };
    if !reachable && !error.is_null() {
        // TODO: return an actual NSError
        env.mem.write(error, nil);
    }
    reachable
}

- (id)description {
    match env.objc.borrow(this) {
        NSURLHostObject::FileURL { ns_string, working_directory } => {
            let working_directory = working_directory.as_str().to_string();
            let mut description = to_rust_string(env, *ns_string).to_string().clone();
            if !description.starts_with('/') {
                description = format!("{} -- file://localhost{}", description.trim_start_matches("./"), working_directory );
            }
            let desc = from_rust_string(env, description);
            autorelease(env, desc)
        },
        NSURLHostObject::OtherURL { ns_string } => *ns_string,
    }
}

- (id)path {
    match *env.objc.borrow(this) {
        NSURLHostObject::FileURL { ns_string, .. } => ns_string,
        NSURLHostObject::OtherURL { ns_string } => {
            let url = to_rust_string(env, ns_string);
            let path = percent_decode(split_url(&url).path);
            let path = from_rust_string(env, path);
            autorelease(env, path)
        },
    }
}

- (id)pathExtension {
    let path: id = msg![env; this path];
    msg![env; path pathExtension]
}

- (id)scheme {
    let ns_string = match *env.objc.borrow(this) {
        NSURLHostObject::FileURL { .. } => return get_static_str(env, "file"),
        NSURLHostObject::OtherURL { ns_string } => ns_string,
    };
    let url = to_rust_string(env, ns_string);
    let scheme = split_url(&url).scheme.map(str::to_string);
    url_component(env, scheme)
}

- (id)host {
    let &NSURLHostObject::OtherURL { ns_string } = env.objc.borrow(this) else {
        return nil;
    };
    let url = to_rust_string(env, ns_string);
    let host = split_url(&url).host().map(percent_decode);
    url_component(env, host)
}

// Unlike path, the query and fragment aren't unescaped.
- (id)query {
    let &NSURLHostObject::OtherURL { ns_string } = env.objc.borrow(this) else {
        return nil;
    };
    let url = to_rust_string(env, ns_string);
    let query = split_url(&url).query.map(str::to_string);
    url_component(env, query)
}

- (id)fragment {
    let &NSURLHostObject::OtherURL { ns_string } = env.objc.borrow(this) else {
        return nil;
    };
    let url = to_rust_string(env, ns_string);
    let fragment = split_url(&url).fragment.map(str::to_string);
    url_component(env, fragment)
}

- (id)absoluteString {
    match *env.objc.borrow(this) {
        // FIXME: don't assume URL is already absolute
        NSURLHostObject::FileURL { ns_string, .. } => ns_string,
        // FIXME: relative URLs (with a base URL) aren't supported, so every
        // other URL is its own absolute string.
        NSURLHostObject::OtherURL { ns_string } => ns_string,
    }
}

- (id)absoluteURL {
    // FIXME: don't assume URL is already absolute
    let &NSURLHostObject::OtherURL { .. } = env.objc.borrow(this) else {
        unimplemented!(); // TODO
    };
    this
}

- (bool)getFileSystemRepresentation:(MutPtr<u8>)buffer
                          maxLength:(NSUInteger)buffer_size {
    let &NSURLHostObject::FileURL { ns_string, .. } = env.objc.borrow(this) else {
        unimplemented!(); // TODO
    };
    msg![env; ns_string getFileSystemRepresentation:buffer maxLength:buffer_size]
}

- (id)URLByAppendingPathComponent:(id)path_component // NSString *
                      isDirectory:(bool)is_directory {
    let &NSURLHostObject::FileURL { ns_string, .. } = env.objc.borrow(this) else {
        unimplemented!(); // TODO
    };
    let mut path: id = msg![env; ns_string stringByAppendingPathComponent:path_component];
    if is_directory {
        path = msg![env; path stringByAppendingString:(get_static_str(env, "/"))];
    }
    msg_class![env; NSURL fileURLWithPath:path]
}

- (id)URLByDeletingLastPathComponent {
    let &NSURLHostObject::FileURL { ns_string, .. } = env.objc.borrow(this) else {
        unimplemented!(); // TODO
    };
    let path: id = msg![env; ns_string stringByDeletingLastPathComponent];
    msg_class![env; NSURL fileURLWithPath:path]
}

// TODO: more constructors, more accessors

@end

// A caching layer a top of NSURL, it's OK to stub
// as we don't have yet a networking support
@implementation NSURLCache: NSObject
+ (id)sharedURLCache {
    // TODO
    nil
}
@end

};

/// Shortcut for host code, provides a view of a URL as a path.
/// TODO: Try to avoid allocating a new GuestPathBuf in more cases.
pub fn to_rust_path(env: &mut Environment, url: id) -> Cow<'static, GuestPath> {
    let path_string: id = msg![env; url path];

    match to_rust_string(env, path_string) {
        Cow::Borrowed(path) => Cow::Borrowed(path.as_ref()),
        Cow::Owned(path_buf) => Cow::Owned(path_buf.into()),
    }
}

/// The parts of a URL string, as split by RFC 3986's regular expression
/// (appendix B). The parts aren't unescaped.
struct UrlParts<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}
impl<'a> UrlParts<'a> {
    /// The host in the authority, without user info or port.
    fn host(&self) -> Option<&'a str> {
        let authority = self.authority?;
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = if let Some(rest) = host.strip_prefix('[') {
            // IPv6 address literal (the brackets aren't part of the host).
            rest.split_once(']').map_or(rest, |(host, _)| host)
        } else {
            host.split_once(':').map_or(host, |(host, _)| host)
        };
        Some(host)
    }
}

fn split_url(url: &str) -> UrlParts<'_> {
    let (rest, fragment) = match url.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (url, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (scheme, rest) = match rest.split_once(':') {
        Some((scheme, rest)) if !scheme.is_empty() && !scheme.contains('/') => (Some(scheme), rest),
        _ => (None, rest),
    };
    let (authority, path) = match rest.strip_prefix("//") {
        Some(rest) => {
            let end = rest.find('/').unwrap_or(rest.len());
            (Some(&rest[..end]), &rest[end..])
        }
        None => (None, rest),
    };
    UrlParts {
        scheme,
        authority,
        path,
        query,
        fragment,
    }
}

/// Replaces `%XX` escapes. Escapes that aren't valid UTF-8 are kept as they
/// are.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| s.to_string())
}

/// An autoreleased `NSString*` for a URL component, or `nil` if the URL
/// doesn't have it.
fn url_component(env: &mut Environment, component: Option<String>) -> id {
    match component {
        Some(component) => {
            let ns_string = from_rust_string(env, component);
            autorelease(env, ns_string)
        }
        None => nil,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_url() {
        let parts = split_url("https://user@example.com:8080/a%20b/c.html?x=1&y=2#top");
        assert_eq!(parts.scheme, Some("https"));
        assert_eq!(parts.host(), Some("example.com"));
        assert_eq!(parts.path, "/a%20b/c.html");
        assert_eq!(percent_decode(parts.path), "/a b/c.html");
        assert_eq!(parts.query, Some("x=1&y=2"));
        assert_eq!(parts.fragment, Some("top"));

        let parts = split_url("nakamap://chat");
        assert_eq!(parts.scheme, Some("nakamap"));
        assert_eq!(parts.host(), Some("chat"));
        assert_eq!(parts.path, "");
        assert_eq!(parts.query, None);

        let parts = split_url("about:blank");
        assert_eq!(parts.scheme, Some("about"));
        assert_eq!(parts.host(), None);
        assert_eq!(parts.path, "blank");

        let parts = split_url("/just/a/path");
        assert_eq!(parts.scheme, None);
        assert_eq!(parts.host(), None);
        assert_eq!(parts.path, "/just/a/path");

        assert_eq!(split_url("http://[::1]:80/").host(), Some("::1"));
    }
}
