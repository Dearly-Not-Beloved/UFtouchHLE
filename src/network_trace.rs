/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Opt-in, observational network tracing. Never sends guest messages to inspect
//! arguments, invokes callbacks, changes errno, or dereferences argument
//! pointers.

use crate::objc::{id, Class, SEL};
use crate::{Environment, ThreadId};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    sequence: u64,
    truncated: bool,
    active: HashMap<ThreadId, Vec<(u64, String)>>,
    callbacks: HashMap<u32, PendingCallbacks>,
}

#[derive(Default)]
struct PendingCallbacks {
    events: Vec<u64>,
    remaining: usize,
}

pub struct Call {
    id: u64,
    thread: ThreadId,
    api: String,
    selector: Option<String>,
    arguments: [u32; 4],
    callback: bool,
}

fn state(env: &mut Environment) -> &mut State {
    &mut env.framework_state.network_trace
}

fn callback_selector(selector: &str) -> bool {
    selector.starts_with("match:")
        || selector.starts_with("session:")
        || selector.starts_with("connection:")
        || matches!(
            selector,
            "connectionDidFinishLoading:" | "localPlayerDidChange:"
        )
}

fn network_class(class: &str) -> bool {
    class.starts_with("GK")
        || matches!(
            class,
            "GameCenterSupport"
                | "ReceiveData"
                | "Reachability"
                | "CBReachability"
                | "LobiViewController"
                | "HexNakamap"
                | "Nakamap"
                | "NKNakamapViewController"
                | "CBAPIConnection"
                | "CBTaggedURLConnection"
                | "ISNetworkOperation"
                | "NSURLConnection"
                | "NSURLRequest"
                | "NSMutableURLRequest"
                | "NSNetService"
                | "NSNetServiceBrowser"
        )
}

fn nil_network_selector(selector: &str) -> bool {
    matches!(
        selector,
        "localPlayer"
            | "authenticateHandler"
            | "setAuthenticateHandler:"
            | "isAuthenticated"
            | "playerID"
            | "expectedPlayerCount"
            | "playerIDs"
            | "findMatchForRequest:withCompletionHandler:"
            | "addPlayersToMatch:matchRequest:completionHandler:"
            | "sendData:toPlayers:withDataMode:error:"
            | "sendDataToAllPlayers:withDataMode:error:"
    ) || callback_selector(selector)
}

fn network_symbol(symbol: &str) -> bool {
    let name = symbol.trim_start_matches('_');
    name.starts_with("SCNetworkReachability")
        || name.starts_with("DNSService")
        || name.starts_with("CFSocket")
        || name.starts_with("CFHost")
        || name.starts_with("CFStream")
        || name.starts_with("CFReadStream")
        || name.starts_with("CFWriteStream")
        || matches!(
            name,
            "socket"
                | "connect"
                | "bind"
                | "listen"
                | "accept"
                | "send"
                | "sendto"
                | "recv"
                | "recvfrom"
                | "select"
                | "poll"
                | "shutdown"
                | "getsockopt"
                | "setsockopt"
                | "getsockname"
                | "getpeername"
                | "getaddrinfo"
                | "freeaddrinfo"
                | "gethostbyname"
                | "gethostent"
                | "getifaddrs"
                | "freeifaddrs"
                | "if_nametoindex"
                | "if_nameindex"
        )
}

/// Sequence numbers and time make ordering explicit even across guest yields.
/// There is one final truncation record. Do not infer absence after it.
pub fn record(env: &mut Environment, event: &str, details: Value) -> Option<u64> {
    if !env.options.network_trace {
        return None;
    }
    let limit = env.options.network_trace_limit;
    let thread = env.current_thread;
    let ms = env.startup_time.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let trace = state(env);
    if trace.truncated {
        return None;
    }
    let event = if trace.sequence >= limit {
        trace.truncated = true;
        "trace_truncated"
    } else {
        event
    };
    trace.sequence += 1;
    let seq = trace.sequence;
    let parent = trace
        .active
        .get(&thread)
        .and_then(|s| s.last())
        .map(|p| p.0);
    let details = if trace.truncated {
        json!({"limit":limit})
    } else {
        details
    };
    echo_no_panic!(
        "[NetworkTrace] {}",
        json!({
            "seq":seq, "ms":ms, "pid":std::process::id(), "thread":thread,
            "parent":parent, "event":event, "details":details
        })
    );
    (!trace.truncated).then_some(seq)
}

fn begin(
    env: &mut Environment,
    api: String,
    selector: Option<String>,
    details: Value,
    event: &str,
) -> Option<Call> {
    let arguments = env.cpu.regs()[..4].try_into().unwrap();
    let thread = env.current_thread;
    let id = record(
        env,
        event,
        json!({"api":api, "args_r0_r3":
            if api == "dispatch Block invocation" { None } else { Some(arguments) },
            "context":details}),
    )?;
    state(env)
        .active
        .entry(thread)
        .or_default()
        .push((id, api.clone()));
    Some(Call {
        id,
        thread,
        api,
        selector,
        arguments,
        callback: event == "callback_invoked",
    })
}

pub fn objc_begin(
    env: &mut Environment,
    receiver: id,
    selector: SEL,
    class: Option<Class>,
) -> Option<Call> {
    if !env.options.network_trace {
        return None;
    }
    let name = selector.as_str(&env.mem);
    let class = class
        .and_then(|c| env.objc.try_get_class_name(c))
        .unwrap_or("nil_or_unknown");
    if !network_class(class)
        && !callback_selector(name)
        && !(receiver.is_null() && nil_network_selector(name))
    {
        return None;
    }
    let name = name.to_owned();
    let api = format!("[{class} {name}]");
    let details = json!({"receiver":receiver.to_bits(), "class":class,
                        "selector":name, "nil_receiver":receiver.is_null()});
    let event = if callback_selector(&name) {
        "callback_invoked"
    } else {
        "api_invoked"
    };
    begin(env, api, Some(name), details, event)
}

pub fn c_begin(env: &mut Environment, symbol: &str) -> Option<Call> {
    if !env.options.network_trace {
        return None;
    }
    let thread = env.current_thread;
    let nested = state(env)
        .active
        .get(&thread)
        .is_some_and(|s| !s.is_empty());
    if !network_symbol(symbol) && !(nested && symbol.starts_with("_dispatch_")) {
        return None;
    }
    begin(
        env,
        symbol.to_owned(),
        None,
        json!({"kind":"C"}),
        "api_invoked",
    )
}

pub fn end(env: &mut Environment, call: Option<Call>) {
    let Some(call) = call else { return };
    let returned = [env.cpu.regs()[0], env.cpu.regs()[1]];
    record(
        env,
        if call.callback {
            "callback_returned"
        } else {
            "api_returned"
        },
        json!({"call":call.id, "api":call.api, "return_r0_r1":returned}),
    );
    if call
        .selector
        .as_deref()
        .is_some_and(|s| matches!(s, "alloc" | "allocWithZone:" | "new"))
        && returned[0] != 0
    {
        record(
            env,
            "object_created",
            json!({"call":call.id,"api":call.api,"object":returned[0]}),
        );
    }
    if call.api == "[GKLocalPlayer localPlayer]" && returned[0] == 0 {
        record(
            env,
            "observed_failure",
            json!({"call":call.id,"reason":"local_player_nil"}),
        );
    }
    if let Some(selector) = call.selector.as_deref() {
        // These exact host setters actually store the scalar.
        if (call.api.starts_with("[GKMatchRequest ")
            && matches!(
                selector,
                "setMinPlayers:"
                    | "setMaxPlayers:"
                    | "setDefaultNumberOfPlayers:"
                    | "setPlayerGroup:"
                    | "setPlayerAttributes:"
            ))
            || (call.api.starts_with("[GKSession ") && selector == "setAvailable:")
        {
            record(
                env,
                "state_written",
                json!({"call":call.id,"setter":selector,"raw_value":call.arguments[2]}),
            );
        }
    } else if c_failure(&call.api, returned[0]) {
        let errno = crate::libc::errno::peek_errno(env);
        record(
            env,
            "network_error",
            json!({"call":call.id,"api":call.api,
            "raw_result":returned[0] as i32,"errno_if_initialized":errno,
            "errno_may_be_stale":true}),
        );
    }
    if let Some(stack) = state(env).active.get_mut(&call.thread) {
        stack.pop();
    }
}

fn c_failure(api: &str, value: u32) -> bool {
    let name = api.trim_start_matches('_');
    if name.starts_with("DNSService") {
        return (value as i32) < 0;
    }
    if name == "getaddrinfo" {
        return value != 0;
    }
    if matches!(
        name,
        "SCNetworkReachabilityGetFlags"
            | "SCNetworkReachabilitySetCallback"
            | "SCNetworkReachabilityScheduleWithRunLoop"
            | "SCNetworkReachabilityUnscheduleFromRunLoop"
            | "CFSocketCreate"
            | "CFHostCreateWithName"
            | "CFHostCreateWithAddress"
            | "gethostbyname"
    ) {
        return value == 0;
    }
    matches!(
        name,
        "socket"
            | "connect"
            | "bind"
            | "listen"
            | "accept"
            | "send"
            | "sendto"
            | "recv"
            | "recvfrom"
            | "select"
            | "poll"
            | "shutdown"
            | "getsockopt"
            | "setsockopt"
            | "getsockname"
            | "getifaddrs"
    ) && value == u32::MAX
}

/// Record the real stub behavior, not a synthetic scheduling or completion.
pub fn callback_not_scheduled(env: &mut Environment, api: &str, callback: u32) {
    if env.options.network_trace {
        record(
            env,
            "callback_not_scheduled",
            json!({"api":api,"callback":callback,"reason":"existing_stub"}),
        );
    }
}

pub fn missing_method(env: &mut Environment, reason: &str) {
    if env.options.network_trace {
        record(env, "dispatch_error", json!({"reason":reason}));
    }
}

pub fn scheduled(env: &mut Environment, block: u32, api: &str) {
    if !env.options.network_trace {
        return;
    }
    let thread = env.current_thread;
    let origin = state(env)
        .active
        .get(&thread)
        .and_then(|s| s.last())
        .map(|p| p.0);
    if let Some(origin) = origin {
        if let Some(id) = record(
            env,
            "callback_scheduled",
            json!({"block":block,"origin_call":origin,"api":api}),
        ) {
            let pending = state(env).callbacks.entry(block).or_default();
            pending.events.push(id);
            pending.remaining += 1;
        }
    }
}

pub fn callback_begin(env: &mut Environment, block: u32) -> Option<Call> {
    if !env.options.network_trace {
        return None;
    }
    let pending = state(env).callbacks.get_mut(&block)?;
    let events = pending.events.clone();
    pending.remaining -= 1;
    if pending.remaining == 0 {
        state(env).callbacks.remove(&block);
    }
    begin(
        env,
        "dispatch Block invocation".to_string(),
        None,
        json!({"block":block,"scheduled_candidates":events,
            "correlation":if events.len() == 1 { "single_pending" } else { "ambiguous_block_reuse" }}),
        "callback_invoked",
    )
}

pub fn release_callback(env: &mut Environment, block: u32) {
    if env.options.network_trace {
        state(env).callbacks.remove(&block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_network_apis_without_audio_and_controller_noise() {
        assert!(network_class("GKMatchRequest"));
        assert!(network_class("GameCenterSupport"));
        assert!(callback_selector("match:didReceiveData:fromPlayer:"));
        assert!(nil_network_selector("setAuthenticateHandler:"));
        assert!(!network_class("AVAudioSession"));
        assert!(!network_class("GCController"));
        assert!(!network_symbol("_objc_msgSend"));
        assert!(network_symbol("_socket"));
        assert!(network_symbol("_SCNetworkReachabilityGetFlags"));
        assert!(network_symbol("_DNSServiceBrowse"));
    }

    #[test]
    fn errors_are_interpreted_by_actual_return_convention() {
        assert!(c_failure("_socket", u32::MAX));
        assert!(!c_failure("_socket", 0));
        assert!(c_failure("_SCNetworkReachabilitySetCallback", 0));
        assert!(!c_failure("_SCNetworkReachabilitySetCallback", 1));
        assert!(c_failure("_DNSServiceBrowse", (-65544_i32) as u32));
        assert!(!c_failure("_recv", 0));
        assert!(!c_failure("_CFStreamCreatePairWithSocketToCFHost", 0));
    }
}
