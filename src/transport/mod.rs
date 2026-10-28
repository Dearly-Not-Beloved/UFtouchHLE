/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Independent LAN transport. No guest pointers, callbacks or runtime access.
//! Starting is explicit: merely loading touchHLE never opens a socket.

mod wire;
mod worker;

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Maximum opaque application message size, including an empty message.
pub const MAX_MESSAGE: usize = 65536;
/// Host plus at most three other players.
pub const MAX_PLAYERS: usize = 4;
/// Shared by the wire decoder and LAN advertisements.
pub const PROTOCOL_VERSION: u16 = 1;
/// Conservative datagram payload cap; no fragmentation or TCP fallback.
pub const MAX_UNRELIABLE_MESSAGE: usize = 1100;

/// Caller-owned identity. Reuse it on reconnect; this is not authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub [u8; 16]);
impl PlayerId {
    /// Generate an independent UUID v4 using the project's existing dependency.
    pub fn random() -> Self {
        Self(*uuid::Uuid::new_v4().as_bytes())
    }
}

/// Fresh per Host lifetime; stale sessions cannot attach to a new room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionId(pub [u8; 16]);

/// The Host only coordinates membership. All payload connections are direct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Client(SocketAddrV4),
}

/// Worker configuration. IPv4 numeric addresses avoid DNS on the caller thread.
#[derive(Clone, Debug)]
pub struct Config {
    pub player: PlayerId,
    pub bind: SocketAddrV4,
    /// Opaque compatibility tag; all members must supply the same bytes.
    pub compatibility: [u8; 16],
    pub unreliable: bool,
    pub max_players: usize,
    pub command_capacity: usize,
    pub event_capacity: usize,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub peer_timeout: Duration,
}
impl Config {
    pub fn new(player: PlayerId, bind: SocketAddrV4, compatibility: [u8; 16]) -> Self {
        Self {
            player,
            bind,
            compatibility,
            unreliable: false,
            max_players: MAX_PLAYERS,
            command_capacity: 64,
            event_capacity: 64,
            connect_timeout: Duration::from_millis(100),
            handshake_timeout: Duration::from_secs(3),
            heartbeat_interval: Duration::from_secs(1),
            peer_timeout: Duration::from_secs(5),
        }
    }

    fn validate(&self, role: Role) -> Result<(), &'static str> {
        if self.player.0 == [0; 16] {
            return Err("zero player identity");
        }
        if !(2..=MAX_PLAYERS).contains(&self.max_players) {
            return Err("invalid player capacity");
        }
        if !(1..=1024).contains(&self.command_capacity)
            || !(1..=1024).contains(&self.event_capacity)
        {
            return Err("queue capacities must be in 1..=1024");
        }
        if self.connect_timeout.is_zero()
            || self.connect_timeout > Duration::from_millis(250)
            || self.handshake_timeout < self.connect_timeout
            || self.heartbeat_interval < Duration::from_millis(10)
            || self.peer_timeout <= self.heartbeat_interval
        {
            return Err("invalid timeout configuration");
        }
        if let Role::Client(address) = role {
            if address.port() == 0 || address.ip().is_unspecified() {
                return Err("client needs a concrete Host address and port");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    ReliableOrdered,
    /// Requires Config::unreliable. Never silently falls back to TCP.
    Unreliable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub id: PlayerId,
    pub address: SocketAddrV4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Io(std::io::ErrorKind),
    Protocol(&'static str),
    Timeout,
    RemoteClosed,
    RoomFull,
    DuplicateIdentity,
    Incompatible,
    PeerUnavailable,
    Backpressure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Requested = 1,
    HostLost = 2,
    EventBackpressure = 3,
    WorkerFailed = 4,
}

/// Owned data only. Consume on the emulator thread in a future adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Listening(SocketAddrV4),
    Joined {
        session: SessionId,
        host: PlayerId,
    },
    Roster(Vec<Peer>),
    PeerConnected(PlayerId),
    Data {
        from: PlayerId,
        bytes: Vec<u8>,
    },
    UnreliableData {
        from: PlayerId,
        bytes: Vec<u8>,
    },
    PeerDisconnected {
        peer: PlayerId,
        reason: Failure,
    },
    Error {
        peer: Option<PlayerId>,
        reason: Failure,
    },
    /// May arrive after buffered events; no subsequent event is produced.
    Stopped(StopReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    QueueFull,
    Stopped,
    TooLarge,
    UnsupportedDelivery,
}

enum Command {
    Send(PlayerId, Vec<u8>),
    Datagram(PlayerId, Vec<u8>),
    Accepting(bool),
}

/// Nonblocking front end. Drop requests shutdown but never joins the worker.
/// Queue acceptance is not a delivery acknowledgement. Drain events regularly.
pub struct Transport {
    commands: mpsc::SyncSender<Command>,
    events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
    reported_stop: bool,
    unreliable: bool,
}
impl Transport {
    /// Validate and spawn only. Bind/connect results arrive asynchronously.
    pub fn start(config: Config, role: Role) -> Result<Self, std::io::Error> {
        config
            .validate(role)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let (commands, receiver) = mpsc::sync_channel(config.command_capacity);
        let (sender, events) = mpsc::sync_channel(config.event_capacity);
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicU8::new(0));
        let worker_stop = stop.clone();
        let worker_status = status.clone();
        let unreliable = config.unreliable;
        std::thread::Builder::new()
            .name("touchHLE LAN transport".into())
            .spawn(move || {
                worker::run(config, role, receiver, sender, worker_stop, worker_status)
            })?;
        Ok(Self {
            commands,
            events,
            stop,
            status,
            reported_stop: false,
            unreliable,
        })
    }

    pub fn try_send(
        &self,
        to: PlayerId,
        bytes: Vec<u8>,
        delivery: Delivery,
    ) -> Result<(), SendError> {
        if delivery == Delivery::Unreliable && !self.unreliable {
            return Err(SendError::UnsupportedDelivery);
        }
        if bytes.len() > MAX_MESSAGE {
            return Err(SendError::TooLarge);
        }
        if delivery == Delivery::Unreliable && bytes.len() > MAX_UNRELIABLE_MESSAGE {
            return Err(SendError::TooLarge);
        }
        if self.stop.load(Ordering::Acquire) || self.status.load(Ordering::Acquire) != 0 {
            return Err(SendError::Stopped);
        }
        self.commands
            .try_send(if delivery == Delivery::Unreliable {
                Command::Datagram(to, bytes)
            } else {
                Command::Send(to, bytes)
            })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => SendError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => SendError::Stopped,
            })
    }

    /// Host-only admission control; clients cannot reopen a Host's room.
    pub fn set_accepting_players(&self, accepting: bool) -> Result<(), SendError> {
        if self.stop.load(Ordering::Acquire) || self.status.load(Ordering::Acquire) != 0 {
            return Err(SendError::Stopped);
        }
        self.commands
            .try_send(Command::Accepting(accepting))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => SendError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => SendError::Stopped,
            })
    }

    /// Never waits for a socket, event or worker exit.
    pub fn poll(&mut self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if !self.reported_stop => {
                self.reported_stop = true;
                let reason = match self.status.load(Ordering::Acquire) {
                    1 => StopReason::Requested,
                    2 => StopReason::HostLost,
                    3 => StopReason::EventBackpressure,
                    _ => StopReason::WorkerFailed,
                };
                Some(Event::Stopped(reason))
            }
            Err(_) => None,
        }
    }

    /// Asynchronous leave. Restart explicitly to rejoin after terminal failure.
    pub fn disconnect(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_configuration_without_starting_network() {
        let mut config = Config::new(PlayerId([1; 16]), "127.0.0.1:0".parse().unwrap(), [7; 16]);
        assert!(config.validate(Role::Host).is_ok());
        assert!(config.validate(Role::Client(config.bind)).is_err());
        config.event_capacity = 0;
        assert!(config.validate(Role::Host).is_err());
        config.event_capacity = 64;
        config.peer_timeout = config.heartbeat_interval;
        assert!(config.validate(Role::Host).is_err());
    }

    #[test]
    fn caller_queue_is_bounded_and_unreliable_is_never_faked() {
        let (commands, receiver) = mpsc::sync_channel(1);
        let (sender, events) = mpsc::sync_channel(1);
        let status = Arc::new(AtomicU8::new(0));
        let mut transport = Transport {
            commands,
            events,
            stop: Arc::new(AtomicBool::new(false)),
            status: status.clone(),
            reported_stop: false,
            unreliable: false,
        };
        let id = PlayerId([1; 16]);
        assert_eq!(
            transport.try_send(id, vec![], Delivery::Unreliable),
            Err(SendError::UnsupportedDelivery)
        );
        assert_eq!(
            transport.try_send(id, vec![0; MAX_MESSAGE + 1], Delivery::ReliableOrdered),
            Err(SendError::TooLarge)
        );
        assert!(transport
            .try_send(id, vec![0, 255], Delivery::ReliableOrdered)
            .is_ok());
        assert_eq!(
            transport.try_send(id, vec![], Delivery::ReliableOrdered),
            Err(SendError::QueueFull)
        );
        let Command::Send(peer, bytes) = receiver.try_recv().unwrap() else {
            panic!("not reliable")
        };
        assert_eq!((peer, bytes), (id, vec![0, 255]));
        assert!(transport.poll().is_none());
        sender
            .try_send(Event::Listening("127.0.0.1:1".parse().unwrap()))
            .unwrap();
        status.store(StopReason::EventBackpressure as u8, Ordering::Release);
        drop(sender);
        assert!(matches!(transport.poll(), Some(Event::Listening(_))));
        assert_eq!(
            transport.poll(),
            Some(Event::Stopped(StopReason::EventBackpressure))
        );
        assert!(transport.poll().is_none());
        assert_eq!(
            transport.try_send(id, vec![], Delivery::ReliableOrdered),
            Err(SendError::Stopped)
        );
    }
}
