/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Local route snapshots for the explicit GameKit backend. No packets are sent.
use crate::{transport::Role, Environment};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct State {
    pending: Option<mpsc::Receiver<bool>>,
    cached: Option<(Instant, bool)>,
}

fn probe(bind: SocketAddrV4, destination: SocketAddrV4) -> bool {
    let result = || -> std::io::Result<bool> {
        let socket = UdpSocket::bind(SocketAddrV4::new(*bind.ip(), 0))?;
        socket.set_broadcast(true)?;
        // UDP connect asks the OS to select a route; it does not send a
        // datagram.
        socket.connect(destination)?;
        Ok(!socket.local_addr()?.ip().is_unspecified())
    };
    result().unwrap_or(false)
}

/// None means disabled or the first route query has not completed. A positive
/// result is only a local route, never proof of a reachable peer or Internet.
pub(crate) fn local_route(env: &mut Environment) -> Option<bool> {
    if !super::local_network::enabled(env) {
        return None;
    }
    let bind = env.options.gamekit_bind;
    let destination = match env.options.gamekit_role {
        Some(Role::Client(address)) => address,
        Some(Role::Host) | None => SocketAddrV4::new(
            if bind.ip().is_unspecified() {
                Ipv4Addr::BROADCAST
            } else {
                *bind.ip()
            },
            9,
        ),
    };
    let s = &mut env.framework_state.game_kit.route;
    if let Some(rx) = &s.pending {
        match rx.try_recv() {
            Ok(value) => {
                s.cached = Some((Instant::now(), value));
                s.pending = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                s.cached = Some((Instant::now(), false));
                s.pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
    }
    if s.pending.is_none()
        && s.cached
            .is_none_or(|(time, _)| time.elapsed() >= Duration::from_secs(1))
    {
        let (tx, rx) = mpsc::sync_channel(1);
        if std::thread::Builder::new()
            .name("GameKit local route".into())
            .spawn(move || {
                let _ = tx.send(probe(bind, destination));
            })
            .is_ok()
        {
            s.pending = Some(rx);
        } else {
            s.cached = Some((Instant::now(), false));
        }
    }
    s.cached.map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_is_not_peer_availability() {
        // No listener is present or necessary for a local route query.
        assert!(probe(
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:9".parse().unwrap()
        ));
        assert!(!probe(
            "192.0.2.123:0".parse().unwrap(),
            "127.0.0.1:9".parse().unwrap()
        ));
    }
}
