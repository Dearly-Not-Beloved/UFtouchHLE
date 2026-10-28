/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Explicit local-only GameKit backend. All guest execution stays on thread 0.
use super::gk_match;
use crate::abi::CallFromHost;
use crate::frameworks::foundation::{ns_array, ns_string};
use crate::libc::dispatch::{_Block_copy, _Block_release, block_invoke_function};
use crate::mem::MutVoidPtr;
use crate::objc::{autorelease, id, msg, msg_class, nil, release, retain};
use crate::transport::{Config, Event, Peer, PlayerId, Transport};
use crate::Environment;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Spec {
    pub min: u32,
    pub max: u32,
    pub default: u32,
    pub group: u32,
    pub attributes: u32,
}
impl Spec {
    pub fn valid(self) -> bool {
        (2..=4).contains(&self.min)
            && self.max >= self.min
            && self.max <= 4
            && (self.default == 0 || (self.min..=self.max).contains(&self.default))
    }
    fn target(self) -> usize {
        if self.default == 0 {
            self.max as usize
        } else {
            self.default as usize
        }
    }
}
struct Pending {
    block: MutVoidPtr,
    add: bool,
    target: usize,
    deadline: Instant,
}
struct Completion {
    block: MutVoidPtr,
    add: bool,
    result: Result<id, (i32, String)>,
    generation: u64,
}
enum Notice {
    Player(PlayerId, u32),
    Data(PlayerId, Vec<u8>),
    Error(String),
}

#[derive(Default)]
pub(super) struct State {
    pub local_player: Option<id>,
    pub identity: Option<PlayerId>,
    pub auth_handler: MutVoidPtr,
    pub authenticated: bool,
    auth_pending: bool,
    node: Option<Transport>,
    pub active: Option<id>,
    connected: BTreeSet<PlayerId>,
    /// Connections already announced to the guest delegate. Physical socket
    /// readiness alone must not erase expectedPlayerCount in the completion.
    announced: BTreeSet<PlayerId>,
    attributes: BTreeMap<PlayerId, u32>,
    roster: Vec<Peer>,
    pending: Option<Pending>,
    completions: VecDeque<Completion>,
    notices: VecDeque<Notice>,
    notice_bytes: usize,
    generation: u64,
    spec: Option<Spec>,
    joined: bool,
    coordinator: Option<PlayerId>,
    local_admission: bool,
    remote_admission: BTreeSet<PlayerId>,
    pumping: bool,
    rooms: super::rooms::State,
}
fn state(env: &mut Environment) -> &mut State {
    &mut env.framework_state.game_kit.local
}
pub(super) fn enabled(env: &Environment) -> bool {
    env.options.network_access && env.options.gamekit_enabled()
}

pub(super) fn identity(env: &mut Environment) -> PlayerId {
    if let Some(id) = state(env).identity {
        return id;
    }
    let id = env
        .options
        .gamekit_player_id
        .unwrap_or_else(PlayerId::random);
    state(env).identity = Some(id);
    id
}
/// Short opaque ASCII alias, with collision checking when installing a roster.
pub(super) fn alias(id: PlayerId) -> String {
    Sha256::digest(id.0)[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn error(env: &mut Environment, code: i32, reason: &str) -> id {
    let domain = ns_string::get_static_str(env, "GKErrorDomain");
    let key = ns_string::get_static_str(env, "NSLocalizedDescriptionKey");
    let description = ns_string::from_rust_string(env, reason.into());
    let info: id = msg_class![env; NSDictionary dictionaryWithObject:description forKey:key];
    release(env, description);
    msg_class![env; NSError errorWithDomain:domain code:code userInfo:info]
}
fn trace(env: &mut Environment, event: &str, block: MutVoidPtr) {
    if env.options.network_trace {
        crate::network_trace::record(
            env,
            event,
            serde_json::json!({"source":"local GameKit","block":block.to_bits()}),
        );
    }
}
pub(super) fn set_auth(env: &mut Environment, handler: id) {
    let handler = _Block_copy(env, handler.cast_void().cast_const());
    let old = std::mem::replace(&mut state(env).auth_handler, handler);
    state(env).auth_pending = !handler.is_null();
    _Block_release(env, old);
    if !handler.is_null() {
        trace(env, "callback_scheduled", handler);
    }
}
fn deferred(env: &mut Environment, block: id, add: bool, result: Result<id, (i32, String)>) {
    let block = _Block_copy(env, block.cast_void().cast_const());
    if let Ok(object) = result {
        retain(env, object);
    }
    let generation = state(env).generation;
    state(env).completions.push_back(Completion {
        block,
        add,
        result,
        generation,
    });
    trace(env, "callback_scheduled", block);
}
pub(super) fn request(
    env: &mut Environment,
    request: Option<Spec>,
    handler: id,
    add: bool,
    match_: id,
) {
    let Some(spec) = request.filter(|s| s.valid()) else {
        deferred(
            env,
            handler,
            add,
            Err((13, "Invalid or unsupported match request".into())),
        );
        return;
    };
    if env.options.network_trace {
        crate::network_trace::record(
            env,
            "gamekit_request",
            serde_json::json!({
                "add":add,"min":spec.min,"max":spec.max,"default":spec.default,
                "group":spec.group,"attributes":spec.attributes,"match":match_.to_bits()
            }),
        );
    }
    if !state(env).authenticated {
        deferred(
            env,
            handler,
            add,
            Err((6, "Local player has not authenticated".into())),
        );
        return;
    }
    if state(env).pending.is_some() || (!add && state(env).active.is_some()) {
        deferred(
            env,
            handler,
            add,
            Err((17, "Another match or request is active".into())),
        );
        return;
    }
    let target = if add {
        let previous = state(env).spec;
        if state(env).active != Some(match_)
            || previous.is_none_or(|old| old.group != spec.group || old.max != spec.max)
        {
            deferred(
                env,
                handler,
                true,
                Err((
                    17,
                    "Add players requires this instance's active match and compatible request"
                        .into(),
                )),
            );
            return;
        }
        let count = state(env).connected.len() + 1;
        let target = spec.target().max(count + 1);
        if target > spec.max as usize {
            deferred(
                env,
                handler,
                true,
                Err((13, "Room already at requested capacity".into())),
            );
            return;
        }
        if !admission(env, true) {
            deferred(
                env,
                handler,
                true,
                Err((3, "Cannot request room admission".into())),
            );
            return;
        }
        state(env).spec = Some(spec);
        let hello = metadata(spec);
        let peers = state(env).connected.iter().copied().collect::<Vec<_>>();
        for peer in peers {
            if let Some(node) = &state(env).node {
                if node
                    .try_send(
                        peer,
                        hello.clone(),
                        crate::transport::Delivery::ReliableOrdered,
                    )
                    .is_err()
                {
                    deferred(
                        env,
                        handler,
                        true,
                        Err((3, "Cannot update matchmaking attributes".into())),
                    );
                    return;
                }
            }
        }
        target
    } else {
        let mut hash = Sha256::new();
        hash.update(b"UF-GK-TCP-UDP-v1\0");
        hash.update(env.bundle.bundle_identifier().as_bytes());
        hash.update([0]);
        hash.update(env.bundle.bundle_version().as_bytes());
        hash.update(spec.group.to_be_bytes());
        hash.update(spec.max.to_be_bytes());
        let mut config = Config::new(
            identity(env),
            env.options.gamekit_bind,
            hash.finalize()[..16].try_into().unwrap(),
        );
        config.unreliable = true;
        config.max_players = spec.max as usize;
        let mut rooms = std::mem::take(&mut state(env).rooms);
        let connection = rooms.start(&env.options, config);
        state(env).rooms = rooms;
        let node = connection.and_then(|connection| {
            connection
                .map(|(config, role)| Transport::start(config, role).map_err(|e| e.to_string()))
                .transpose()
        });
        match node {
            Ok(node) => state(env).node = node,
            Err(e) => {
                deferred(
                    env,
                    handler,
                    false,
                    Err((3, format!("Transport startup: {e}"))),
                );
                return;
            }
        }
        let s = state(env);
        s.generation += 1;
        s.connected.clear();
        s.announced.clear();
        s.attributes.clear();
        s.roster.clear();
        s.joined = false;
        s.coordinator = None;
        s.local_admission = true;
        s.remote_admission.clear();
        s.spec = Some(spec);
        spec.target()
    };
    let block = _Block_copy(env, handler.cast_void().cast_const());
    let deadline = Instant::now() + env.options.gamekit_timeout;
    state(env).pending = Some(Pending {
        block,
        add,
        target,
        deadline,
    });
    trace(env, "callback_scheduled", block);
}

fn complete(env: &mut Environment, result: Result<id, (i32, String)>) {
    let Some(pending) = state(env).pending.take() else {
        return;
    };
    if let Ok(object) = result {
        retain(env, object);
    }
    let generation = state(env).generation;
    state(env).completions.push_back(Completion {
        block: pending.block,
        add: pending.add,
        result,
        generation,
    });
}
pub(super) fn cancel(env: &mut Environment) {
    let Some(add) = state(env).pending.as_ref().map(|p| p.add) else {
        return;
    };
    complete(env, Err((2, "Local matchmaking cancelled".into())));
    if add {
        admission(env, false);
    } else {
        reset_network(env);
    }
}
fn reset_network(env: &mut Environment) {
    let s = state(env);
    s.coordinator = None;
    s.local_admission = false;
    s.remote_admission.clear();
    s.announced.clear();
    s.generation += 1;
    s.node = None;
    s.joined = false;
    s.connected.clear();
    s.attributes.clear();
    s.roster.clear();
    s.notices.clear();
    s.notice_bytes = 0;
    s.rooms = super::rooms::State::default();
}
pub(super) fn disconnect(env: &mut Environment, match_: id) {
    if state(env).active != Some(match_) {
        return;
    }
    complete(
        env,
        Err((2, "Match disconnected during matchmaking".into())),
    );
    reset_network(env);
    state(env).active = None;
    env.objc.borrow_mut::<gk_match::Match>(match_).disconnected = true;
    release(env, match_);
}
pub(super) fn finish(env: &mut Environment, match_: id) {
    if state(env).active == Some(match_) {
        admission(env, false);
    }
}

fn admission(env: &mut Environment, open: bool) -> bool {
    state(env).local_admission = open;
    let s = state(env);
    if let Err(error) = s.rooms.admission(open || !s.remote_admission.is_empty()) {
        log!("[LocalGameKit] {error}");
    }
    let Some(node) = s.node.as_ref() else {
        return false;
    };
    if let Some(host) = s.coordinator.filter(|host| Some(*host) != s.identity) {
        node.try_send(
            host,
            vec![b'G', b'K', b'U', b'1', 3, u8::from(open)],
            crate::transport::Delivery::ReliableOrdered,
        )
        .is_ok()
    } else {
        node.set_accepting_players(open || !s.remote_admission.is_empty())
            .is_ok()
    }
}

fn metadata(spec: Spec) -> Vec<u8> {
    let mut hello = b"GKU1\x01".to_vec();
    hello.extend_from_slice(&spec.group.to_be_bytes());
    hello.extend_from_slice(&spec.attributes.to_be_bytes());
    hello.push(spec.max as u8);
    hello
}
pub(super) fn expected(env: &mut Environment, match_: id) -> u32 {
    if state(env).active != Some(match_) {
        return 0;
    }
    let count = state_count(env);
    let s = state(env);
    let waiting_notice = s.connected.difference(&s.announced).count();
    state(env)
        .pending
        .as_ref()
        .map_or(0, |p| p.target.saturating_sub(count) as u32)
        .max(waiting_notice as u32)
}
fn state_count(env: &Environment) -> usize {
    env.framework_state.game_kit.local.connected.len() + 1
}
pub(super) fn player_ids(env: &mut Environment, match_: id) -> id {
    let players = if state(env).active == Some(match_) {
        state(env).announced.iter().copied().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let ids = players
        .into_iter()
        .map(|p| ns_string::from_rust_string(env, alias(p)))
        .collect();
    let array = ns_array::from_vec(env, ids);
    autorelease(env, array)
}
pub(super) fn send(
    env: &mut Environment,
    match_: id,
    data: id,
    players: Option<id>,
    mode: u32,
) -> Result<(), (i32, String)> {
    if state(env).active != Some(match_) || env.objc.borrow::<gk_match::Match>(match_).disconnected
    {
        return Err((3, "Match is disconnected".into()));
    }
    if data == nil || mode > 1 {
        return Err((17, "Invalid data or send mode".into()));
    }
    let size: u32 = msg![env; data length];
    let limit = if mode == 0 {
        crate::transport::MAX_MESSAGE
    } else {
        crate::transport::MAX_UNRELIABLE_MESSAGE
    } - 5;
    if size as usize > limit {
        return Err((17, format!("Payload exceeds {limit} byte limit")));
    }
    let ptr: crate::mem::ConstVoidPtr = msg![env; data bytes];
    let bytes = if size == 0 {
        Vec::new()
    } else {
        env.mem.bytes_at(ptr.cast(), size).to_vec()
    };
    let mut targets = state(env).connected.clone();
    if let Some(array) = players {
        if array == nil {
            return Err((17, "nil recipient list".into()));
        }
        let count: u32 = msg![env; array count];
        if count > 3 {
            return Err((17, "Too many recipients".into()));
        }
        targets.clear();
        for i in 0..count {
            let string: id = msg![env; array objectAtIndex:i];
            let name = ns_string::to_rust_string(env, string).into_owned();
            let peer = state(env)
                .connected
                .iter()
                .find(|p| alias(**p) == name)
                .copied()
                .ok_or((17, "Unknown recipient".into()))?;
            targets.insert(peer);
        }
    }
    if targets.is_empty() {
        return Err((17, "Empty recipient list".into()));
    }
    let delivery = if mode == 0 {
        crate::transport::Delivery::ReliableOrdered
    } else {
        crate::transport::Delivery::Unreliable
    };
    let mut framed = b"GKU1\x02".to_vec();
    framed.extend_from_slice(&bytes);
    let node = state(env)
        .node
        .as_ref()
        .ok_or((3, "Transport stopped".into()))?;
    for peer in targets {
        node.try_send(peer, framed.clone(), delivery)
            .map_err(|e| (3, format!("Send rejected: {e:?}")))?;
    }
    Ok(())
}
fn receive(env: &mut Environment, peer: PlayerId, bytes: Vec<u8>, unreliable: bool) {
    if bytes.len() < 5 || &bytes[..4] != b"GKU1" {
        notice(
            env,
            Notice::Error("Invalid GameKit adapter envelope".into()),
        );
        return;
    }
    if bytes[4] == 1 && !unreliable && bytes.len() == 14 {
        let Some(spec) = state(env).spec else {
            return;
        };
        let group = u32::from_be_bytes(bytes[5..9].try_into().unwrap());
        if group != spec.group || bytes[13] != spec.max as u8 {
            notice(env, Notice::Error("Incompatible peer metadata".into()));
            return;
        }
        let attributes = u32::from_be_bytes(bytes[9..13].try_into().unwrap());
        state(env).attributes.insert(peer, attributes);
        if state(env).connected.insert(peer) {
            notice(env, Notice::Player(peer, 1));
        }
    } else if bytes[4] == 2 && state(env).connected.contains(&peer) {
        notice(env, Notice::Data(peer, bytes[5..].to_vec()));
    } else if bytes[4] == 3
        && !unreliable
        && bytes.len() == 6
        && bytes[5] <= 1
        && state(env).rooms.is_host()
        && state(env).connected.contains(&peer)
    {
        if bytes[5] == 1 {
            state(env).remote_admission.insert(peer);
        } else {
            state(env).remote_admission.remove(&peer);
        }
        let local = state(env).local_admission;
        admission(env, local);
    }
}
fn mask(attributes: u32) -> u32 {
    if attributes == 0 {
        u32::MAX
    } else {
        attributes
    }
}
fn notice(env: &mut Environment, notice: Notice) {
    let bytes = match &notice {
        Notice::Data(_, bytes) => bytes.len(),
        _ => 0,
    };
    if state(env).notices.len() >= 64 || state(env).notice_bytes + bytes > 1024 * 1024 {
        complete(env, Err((3, "GameKit callback queue overflow".into())));
        state(env).node = None;
        state(env).connected.clear();
        state(env).joined = false;
        if let Some(active) = state(env).active {
            env.objc.borrow_mut::<gk_match::Match>(active).disconnected = true;
        }
        state(env).notices.clear();
        state(env).notice_bytes = 0;
        state(env).notices.push_back(Notice::Error(
            "GameKit callback queue overflow; transport stopped".into(),
        ));
        return;
    }
    state(env).notice_bytes += bytes;
    state(env).notices.push_back(notice);
}

/// Called by the existing main dispatch drain (also usable in headless tests).
pub(crate) fn pump(env: &mut Environment) {
    if env.current_thread != 0 || !enabled(env) || state(env).pumping {
        return;
    }
    state(env).pumping = true;
    if std::mem::take(&mut state(env).auth_pending) {
        state(env).authenticated = true;
        let handler = state(env).auth_handler;
        let block = _Block_copy(env, handler.cast_const());
        if !block.is_null() {
            trace(env, "callback_invoked", block);
            let _: () = block_invoke_function(env, block.cast_const())
                .call_from_host(env, (block, nil, nil));
            trace(env, "callback_returned", block);
            _Block_release(env, block);
            let center: id = msg_class![env; NSNotificationCenter defaultCenter];
            let name = ns_string::get_static_str(
                env,
                super::gk_local_player::GKPlayerAuthenticationDidChangeNotificationName,
            );
            let local = state(env).local_player.unwrap_or(nil);
            let _: () = msg![env; center postNotificationName:name object:local];
        }
    }
    if let Some(connection) = state(env).rooms.poll() {
        match connection
            .and_then(|(config, role)| Transport::start(config, role).map_err(|e| e.to_string()))
        {
            Ok(node) => {
                // In automatic mode, this can replace a room nobody had
                // joined yet with another instance's room.
                let s = state(env);
                s.node = Some(node);
                s.joined = false;
                s.coordinator = None;
                s.connected.clear();
                s.announced.clear();
                s.attributes.clear();
                s.roster.clear();
                s.remote_admission.clear();
            }
            Err(error) => {
                complete(env, Err((3, error)));
                reset_network(env);
            }
        }
    }
    for _ in 0..64 {
        let event = state(env).node.as_mut().and_then(Transport::poll);
        let Some(event) = event else {
            break;
        };
        let s = state(env);
        let accepting = s.local_admission || !s.remote_admission.is_empty();
        if let Err(error) = s.rooms.observe(&event, accepting) {
            complete(env, Err((3, error)));
            reset_network(env);
            break;
        }
        if env.options.network_trace {
            let details = match &event {
                Event::Data { from, bytes } => {
                    serde_json::json!({"kind":"data","peer":alias(*from),"bytes":bytes.len(),"delivery":"reliable"})
                }
                Event::UnreliableData { from, bytes } => {
                    serde_json::json!({"kind":"data","peer":alias(*from),"bytes":bytes.len(),"delivery":"unreliable"})
                }
                _ => serde_json::json!({"kind":"state","event":format!("{event:?}")}),
            };
            crate::network_trace::record(env, "gamekit_transport", details);
        }
        match event {
            Event::Listening(address) => {
                log!("[LocalGameKit] listening {address}");
            }
            Event::Joined { host, .. } => {
                state(env).joined = true;
                state(env).coordinator = Some(host);
            }
            Event::Roster(peers) => {
                let aliases: BTreeSet<_> = peers.iter().map(|p| alias(p.id)).collect();
                if aliases.len() != peers.len() {
                    complete(env, Err((3, "Player alias collision".into())));
                    reset_network(env);
                    break;
                }
                state(env).roster = peers;
            }
            Event::PeerConnected(peer) => {
                let spec = state(env).spec.unwrap();
                let hello = metadata(spec);
                if let Some(node) = &state(env).node {
                    if let Err(e) =
                        node.try_send(peer, hello, crate::transport::Delivery::ReliableOrdered)
                    {
                        complete(env, Err((3, format!("Metadata send: {e:?}"))));
                        reset_network(env);
                        break;
                    }
                }
            }
            Event::PeerDisconnected { peer, .. } => {
                if state(env).remote_admission.remove(&peer) {
                    let local = state(env).local_admission;
                    admission(env, local);
                }
                state(env).attributes.remove(&peer);
                if state(env).connected.remove(&peer) {
                    notice(env, Notice::Player(peer, 2));
                }
            }
            Event::Data { from, bytes } => receive(env, from, bytes, false),
            Event::UnreliableData { from, bytes } => receive(env, from, bytes, true),
            Event::Error { peer, reason } => {
                if peer.is_some() {
                    log!("[LocalGameKit] peer operation: {reason:?}");
                    continue;
                }
                if state(env).pending.is_some() {
                    complete(env, Err((3, format!("Transport: {reason:?}"))));
                    if state(env).active.is_none() {
                        reset_network(env);
                    }
                } else {
                    notice(env, Notice::Error(format!("Transport: {reason:?}")));
                }
            }
            Event::Stopped(reason) => {
                complete(env, Err((3, format!("Transport stopped: {reason:?}"))));
                let remaining = state(env).connected.iter().copied().collect::<Vec<_>>();
                for peer in remaining {
                    notice(env, Notice::Player(peer, 2));
                }
                notice(env, Notice::Error(format!("Transport stopped: {reason:?}")));
                state(env).node = None;
                state(env).connected.clear();
                state(env).attributes.clear();
                state(env).joined = false;
                if let Some(active) = state(env).active {
                    env.objc.borrow_mut::<gk_match::Match>(active).disconnected = true;
                }
            }
        }
        // Deliver each event before applying a later disconnect to the state.
        deliver_notices(env, 64);
    }
    if let Some(pending) = &state(env).pending {
        let expired = Instant::now() >= pending.deadline;
        let target = pending.target;
        let s = &env.framework_state.game_kit.local;
        let ready = s.joined
            && s.connected.len() + 1 >= target
            && s.attributes
                .values()
                .fold(mask(s.spec.unwrap().attributes), |bits, value| {
                    bits | mask(*value)
                })
                == u32::MAX
            && s.roster
                .iter()
                .all(|p| Some(p.id) == s.identity || s.connected.contains(&p.id));
        if expired {
            complete(env, Err((3, "Local matchmaking timed out".into())));
            if state(env).active.is_none() {
                reset_network(env);
            }
        } else if ready {
            let match_ = if let Some(active) = state(env).active {
                active
            } else {
                let object: id = msg_class![env; GKMatch new];
                state(env).active = Some(object);
                object
            };
            complete(env, Ok(match_));
        }
    }
    let completions = std::mem::take(&mut state(env).completions);
    for completion in completions {
        let old_object = completion.result.as_ref().ok().copied();
        let result = if completion.generation != state(env).generation && completion.result.is_ok()
        {
            Err((2, "Operation invalidated before callback".into()))
        } else {
            completion.result
        };
        let (object, error) = match result {
            Ok(object) => (object, nil),
            Err((code, reason)) => {
                if env.options.network_trace {
                    crate::network_trace::record(
                        env,
                        "gamekit_completion_error",
                        serde_json::json!({"add":completion.add,"code":code,"reason":reason}),
                    );
                }
                (nil, self::error(env, code, &reason))
            }
        };
        if !completion.block.is_null() {
            retain(env, error);
            trace(env, "callback_invoked", completion.block);
            let invoke = block_invoke_function(env, completion.block.cast_const());
            if completion.add {
                let _: () = invoke.call_from_host(env, (completion.block, error));
            } else {
                let _: () = invoke.call_from_host(env, (completion.block, object, error));
            }
            trace(env, "callback_returned", completion.block);
            release(env, error);
        }
        _Block_release(env, completion.block);
        if let Some(object) = old_object {
            release(env, object);
        }
    }
    deliver_notices(env, 64);
    state(env).pumping = false;
}

fn deliver_notices(env: &mut Environment, budget: usize) {
    let generation = state(env).generation;
    for _ in 0..budget {
        let Some(active) = state(env).active else {
            break;
        };
        let delegate = env.objc.borrow::<gk_match::Match>(active).delegate;
        if delegate == nil {
            break;
        }
        if env.objc.get_host_object(delegate).is_none() {
            env.objc.borrow_mut::<gk_match::Match>(active).delegate = nil;
            break;
        }
        let Some(notice) = state(env).notices.pop_front() else {
            break;
        };
        retain(env, active);
        retain(env, delegate);
        match notice {
            Notice::Player(peer, status) => {
                if status == 1 {
                    state(env).announced.insert(peer);
                } else {
                    state(env).announced.remove(&peer);
                }
                let selector = env
                    .objc
                    .register_host_selector("match:player:didChangeState:".into(), &mut env.mem);
                let responds: bool = msg![env; delegate respondsToSelector:selector];
                if responds {
                    let string = ns_string::from_rust_string(env, alias(peer));
                    let _: () =
                        msg![env; delegate match:active player:string didChangeState:status];
                    release(env, string);
                }
            }
            Notice::Data(peer, bytes) => {
                state(env).notice_bytes -= bytes.len();
                let selector = env.objc.register_host_selector(
                    "match:didReceiveData:fromPlayer:".into(),
                    &mut env.mem,
                );
                let responds: bool = msg![env; delegate respondsToSelector:selector];
                if responds {
                    let ptr = env.mem.alloc(bytes.len() as u32);
                    env.mem
                        .bytes_at_mut(ptr.cast(), bytes.len() as u32)
                        .copy_from_slice(&bytes);
                    let data: id = msg_class![env; NSData alloc];
                    let length = bytes.len() as u32;
                    let data: id = msg![env; data initWithBytesNoCopy:ptr length:length];
                    let string = ns_string::from_rust_string(env, alias(peer));
                    let _: () =
                        msg![env; delegate match:active didReceiveData:data fromPlayer:string];
                    release(env, string);
                    release(env, data);
                }
            }
            Notice::Error(reason) => {
                let selector = env
                    .objc
                    .register_host_selector("match:didFailWithError:".into(), &mut env.mem);
                let responds: bool = msg![env; delegate respondsToSelector:selector];
                if responds {
                    let error = error(env, 3, &reason);
                    retain(env, error);
                    let _: () = msg![env; delegate match:active didFailWithError:error];
                    release(env, error);
                }
            }
        }
        release(env, delegate);
        release(env, active);
        if state(env).generation != generation {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specs_and_aliases_are_bounded_and_deterministic() {
        let s = Spec {
            min: 2,
            max: 4,
            default: 2,
            group: 1,
            attributes: 0,
        };
        assert!(s.valid());
        assert_eq!(s.target(), 2);
        assert!(!Spec { min: 1, ..s }.valid());
        assert!(!Spec { default: 5, ..s }.valid());
        assert_eq!(alias(PlayerId([1; 16])), alias(PlayerId([1; 16])));
        assert_eq!(alias(PlayerId([1; 16])).len(), 16);
        assert_ne!(alias(PlayerId([1; 16])), alias(PlayerId([2; 16])));
        assert_eq!(mask(0xffff0000) | mask(0x0000ffff), u32::MAX);
        assert_ne!(mask(0x0000ffff) | mask(0x0000ffff), u32::MAX);
        assert_eq!(mask(0), u32::MAX);
    }
}
