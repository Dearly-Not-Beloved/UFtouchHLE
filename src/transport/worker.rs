/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! One private worker owns every socket. No worker borrows Environment.
use super::wire::{self, Frame};
use super::{Command, Config, Event, Failure, Peer, PlayerId, Role, SessionId, StopReason};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 8;
const MAX_OUT_BYTES: usize = 1024 * 1024;
const MAX_OUT_FRAMES: usize = 256;
const IO_BUDGET: usize = 32 * 1024;

#[derive(Clone, Copy)]
enum Phase {
    Incoming,
    Welcome,
    Peer(PlayerId),
    Ready(PlayerId),
    Rejecting,
}

struct Connection {
    key: u64,
    stream: TcpStream,
    remote: SocketAddrV4,
    phase: Phase,
    decoder: wire::Decoder,
    out: VecDeque<Vec<u8>>,
    offset: usize,
    out_bytes: usize,
    sequence: u64,
    born: Instant,
    received: Instant,
    ping: Instant,
    failure: Option<Failure>,
}
impl Connection {
    fn new(key: u64, stream: TcpStream, phase: Phase) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let SocketAddr::V4(remote) = stream.peer_addr()? else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPv4 only"));
        };
        let now = Instant::now();
        Ok(Self {
            key,
            stream,
            remote,
            phase,
            decoder: Default::default(),
            out: VecDeque::new(),
            offset: 0,
            out_bytes: 0,
            sequence: 0,
            born: now,
            received: now,
            ping: now,
            failure: None,
        })
    }

    fn queue(&mut self, kind: u8, bytes: &[u8]) -> Result<(), Failure> {
        if self.out_bytes + wire::HEADER + bytes.len() > MAX_OUT_BYTES
            || self.out.len() >= MAX_OUT_FRAMES
        {
            return Err(Failure::Backpressure);
        }
        let sequence = self.sequence;
        self.sequence = sequence
            .checked_add(1)
            .ok_or(Failure::Protocol("sequence exhausted"))?;
        let bytes = wire::encode(kind, sequence, bytes);
        self.out_bytes += bytes.len();
        self.out.push_back(bytes);
        Ok(())
    }

    fn control(&mut self, kind: u8, bytes: &[u8]) {
        if let Err(error) = self.queue(kind, bytes) {
            self.failure = Some(error);
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut budget = IO_BUDGET;
        while let Some(bytes) = self.out.front() {
            if budget == 0 {
                break;
            }
            let end = bytes.len().min(self.offset + budget);
            match self.stream.write(&bytes[self.offset..end]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(size) => {
                    self.offset += size;
                    self.out_bytes -= size;
                    budget -= size;
                    if self.offset == bytes.len() {
                        self.out.pop_front();
                        self.offset = 0;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn read(&mut self) -> Result<Vec<Frame>, Failure> {
        let mut frames = Vec::new();
        let mut budget = IO_BUDGET;
        loop {
            while frames.len() < 32 {
                let Some(frame) = self.decoder.next()? else {
                    break;
                };
                self.received = Instant::now();
                frames.push(frame);
            }
            if frames.len() == 32 || budget == 0 {
                break;
            }
            let available =
                (wire::HEADER + super::MAX_MESSAGE).saturating_sub(self.decoder.buffer.len());
            if available == 0 {
                return Err(Failure::Protocol("input buffer full"));
            }
            let mut chunk = [0; 4096];
            let size = chunk.len().min(budget).min(available);
            match self.stream.read(&mut chunk[..size]) {
                Ok(0) => {
                    // Deliver already decoded messages before observing EOF.
                    self.failure = Some(if self.decoder.buffer.is_empty() {
                        Failure::RemoteClosed
                    } else {
                        Failure::Protocol("truncated frame")
                    });
                    break;
                }
                Ok(size) => {
                    self.decoder.buffer.extend_from_slice(&chunk[..size]);
                    budget -= size;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Failure::Io(e.kind())),
            }
        }
        Ok(frames)
    }
}

struct Worker {
    config: Config,
    role: Role,
    listener: TcpListener,
    address: SocketAddrV4,
    session: Option<SessionId>,
    host: Option<PlayerId>,
    members: BTreeMap<PlayerId, Peer>,
    connections: Vec<Connection>,
    retry: BTreeMap<PlayerId, Instant>,
    next_key: u64,
    events: mpsc::SyncSender<Event>,
    reason: Option<StopReason>,
    udp: Option<UdpSocket>,
    accepting: bool,
}
impl Worker {
    fn emit(&mut self, event: Event) {
        if self.reason.is_none() && self.events.try_send(event).is_err() {
            self.reason = Some(StopReason::EventBackpressure);
        }
    }

    fn add(&mut self, stream: TcpStream, phase: Phase) -> Result<Connection, Failure> {
        self.next_key += 1;
        Connection::new(self.next_key, stream, phase).map_err(|e| Failure::Io(e.kind()))
    }

    fn dial(&mut self, address: SocketAddrV4, phase: Phase) -> Result<Connection, Failure> {
        let stream =
            TcpStream::connect_timeout(&SocketAddr::V4(address), self.config.connect_timeout)
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::TimedOut {
                        Failure::Timeout
                    } else {
                        Failure::Io(e.kind())
                    }
                })?;
        self.add(stream, phase)
    }

    fn roster(&self) -> Vec<Peer> {
        self.members.values().cloned().collect()
    }

    fn announce(&mut self) {
        let peers = self.roster();
        let bytes = wire::roster(&peers);
        for connection in &mut self.connections {
            if matches!(connection.phase, Phase::Ready(_)) {
                connection.control(wire::ROSTER, &bytes);
            }
        }
        self.emit(Event::Roster(peers));
    }

    fn reject(connection: &mut Connection, code: u8) {
        connection.control(wire::REJECT, &[code]);
        connection.phase = Phase::Rejecting;
    }

    fn has_peer(&self, id: PlayerId) -> bool {
        self.connections
            .iter()
            .any(|c| matches!(c.phase, Phase::Ready(p) | Phase::Peer(p) if p == id))
    }

    fn accept_join(&mut self, c: &mut Connection, bytes: &[u8]) -> Result<(), Failure> {
        if self.role != Role::Host || bytes.len() != 34 {
            return Err(Failure::Protocol("unexpected Join"));
        }
        let id = PlayerId(bytes[..16].try_into().unwrap());
        let port = u16::from_be_bytes(bytes[16..18].try_into().unwrap());
        if id.0 == [0; 16] || port == 0 {
            return Err(Failure::Protocol("bad Join identity/port"));
        }
        if bytes[18..] != self.config.compatibility {
            Self::reject(c, 3);
            return Ok(());
        }
        if self.members.contains_key(&id) {
            Self::reject(c, 2);
            return Ok(());
        }
        if !self.accepting || self.members.len() >= self.config.max_players {
            Self::reject(c, 1);
            return Ok(());
        }
        self.members.insert(
            id,
            Peer {
                id,
                address: SocketAddrV4::new(*c.remote.ip(), port),
            },
        );
        c.phase = Phase::Ready(id);
        let mut welcome = self.session.unwrap().0.to_vec();
        welcome.extend_from_slice(&self.config.player.0);
        welcome.extend_from_slice(&wire::roster(&self.roster()));
        c.control(wire::WELCOME, &welcome);
        self.emit(Event::PeerConnected(id));
        self.announce();
        Ok(())
    }

    fn install_roster(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        let mut peers = wire::parse_roster(bytes)?;
        let host = self.host.ok_or(Failure::Protocol("no Host"))?;
        if !peers.iter().any(|p| p.id == host) || !peers.iter().any(|p| p.id == self.config.player)
        {
            return Err(Failure::Protocol("Host or self missing from roster"));
        }
        for peer in &mut peers {
            if peer.id == host {
                if let Role::Client(address) = self.role {
                    peer.address = address;
                }
            } else if peer.address.ip().is_unspecified() {
                return Err(Failure::Protocol("unspecified peer address"));
            }
        }
        self.members = peers.iter().map(|p| (p.id, p.clone())).collect();
        for c in &mut self.connections {
            if let Phase::Ready(id) | Phase::Peer(id) = c.phase {
                if !self.members.contains_key(&id) {
                    c.failure = Some(Failure::RemoteClosed);
                }
            }
        }
        self.retry.retain(|id, _| self.members.contains_key(id));
        self.emit(Event::Roster(peers));
        Ok(())
    }

    fn hello(&self) -> Vec<u8> {
        let mut bytes = self.config.player.0.to_vec();
        bytes.extend_from_slice(&self.session.unwrap().0);
        bytes.extend_from_slice(&self.config.compatibility);
        bytes
    }

    fn peer_hello(&mut self, c: &mut Connection, bytes: &[u8]) -> Result<(), Failure> {
        if self.role == Role::Host || bytes.len() != 48 {
            return Err(Failure::Protocol("unexpected PeerHello"));
        }
        let id = PlayerId(bytes[..16].try_into().unwrap());
        if Some(SessionId(bytes[16..32].try_into().unwrap())) != self.session
            || bytes[32..] != self.config.compatibility
            || !self.members.contains_key(&id)
            || Some(id) == self.host
            || id == self.config.player
            || self.has_peer(id)
        {
            return Err(Failure::Protocol("invalid peer/session"));
        }
        match c.phase {
            Phase::Incoming if id > self.config.player => {
                c.control(wire::PEER_HELLO, &self.hello())
            }
            Phase::Peer(expected) if expected == id && id < self.config.player => (),
            _ => return Err(Failure::Protocol("unexpected peer direction")),
        }
        c.phase = Phase::Ready(id);
        self.retry.remove(&id);
        self.emit(Event::PeerConnected(id));
        Ok(())
    }

    fn frame(&mut self, c: &mut Connection, frame: Frame) -> Result<(), Failure> {
        match (c.phase, frame.kind) {
            (Phase::Incoming, wire::JOIN) => self.accept_join(c, &frame.bytes),
            (Phase::Incoming | Phase::Peer(_), wire::PEER_HELLO) => {
                self.peer_hello(c, &frame.bytes)
            }
            (Phase::Welcome, wire::WELCOME) if frame.bytes.len() >= 33 => {
                let session = SessionId(frame.bytes[..16].try_into().unwrap());
                let host = PlayerId(frame.bytes[16..32].try_into().unwrap());
                if session.0 == [0; 16] || host.0 == [0; 16] || host == self.config.player {
                    return Err(Failure::Protocol("bad Welcome identity"));
                }
                self.session = Some(session);
                self.host = Some(host);
                self.install_roster(&frame.bytes[32..])?;
                c.phase = Phase::Ready(host);
                self.emit(Event::Joined { session, host });
                self.emit(Event::PeerConnected(host));
                Ok(())
            }
            (Phase::Welcome, wire::REJECT) if frame.bytes.len() == 1 => Err(match frame.bytes[0] {
                1 => Failure::RoomFull,
                2 => Failure::DuplicateIdentity,
                3 => Failure::Incompatible,
                _ => Failure::Protocol("unknown rejection"),
            }),
            (Phase::Ready(id), wire::ROSTER)
                if Some(id) == self.host && self.role != Role::Host =>
            {
                self.install_roster(&frame.bytes)
            }
            (Phase::Ready(id), wire::DATA) if self.members.contains_key(&id) => {
                self.emit(Event::Data {
                    from: id,
                    bytes: frame.bytes,
                });
                Ok(())
            }
            (Phase::Ready(_), wire::PING) if frame.bytes.is_empty() => {
                c.control(wire::PONG, &[]);
                Ok(())
            }
            (Phase::Ready(_), wire::PONG) if frame.bytes.is_empty() => Ok(()),
            _ => Err(Failure::Protocol("frame invalid for connection state")),
        }
    }

    fn cleanup(&mut self) {
        let mut removed_member = false;
        let mut index = 0;
        while index < self.connections.len() {
            if self.connections[index].failure.is_none() {
                index += 1;
                continue;
            }
            let c = self.connections.remove(index);
            let reason = c.failure.unwrap();
            match c.phase {
                Phase::Ready(peer) => {
                    self.emit(Event::PeerDisconnected { peer, reason });
                    if self.role == Role::Host {
                        self.members.remove(&peer);
                        removed_member = true;
                    } else if Some(peer) == self.host {
                        self.reason.get_or_insert(StopReason::HostLost);
                    } else {
                        self.retry
                            .insert(peer, Instant::now() + Duration::from_millis(200));
                    }
                }
                Phase::Welcome => {
                    self.emit(Event::Error { peer: None, reason });
                    self.reason.get_or_insert(StopReason::HostLost);
                }
                Phase::Peer(id) => {
                    self.emit(Event::Error {
                        peer: Some(id),
                        reason,
                    });
                    self.retry
                        .insert(id, Instant::now() + Duration::from_millis(200));
                }
                Phase::Incoming => self.emit(Event::Error { peer: None, reason }),
                Phase::Rejecting => (),
            }
        }
        if removed_member {
            self.announce();
        }
    }

    fn tick(&mut self) {
        self.datagrams();
        for _ in 0..MAX_CONNECTIONS {
            match self.listener.accept() {
                Ok((stream, _)) if self.connections.len() < MAX_CONNECTIONS => {
                    match self.add(stream, Phase::Incoming) {
                        Ok(c) => self.connections.push(c),
                        Err(reason) => self.emit(Event::Error { peer: None, reason }),
                    }
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.emit(Event::Error {
                        peer: None,
                        reason: Failure::Io(e.kind()),
                    });
                    self.reason.get_or_insert(StopReason::WorkerFailed);
                    break;
                }
            }
        }
        let mut received = Vec::new();
        for c in &mut self.connections {
            if c.failure.is_some() {
                continue;
            }
            if let Err(e) = c.flush() {
                c.failure = Some(Failure::Io(e.kind()));
                continue;
            }
            if matches!(c.phase, Phase::Rejecting) {
                if c.out.is_empty() {
                    c.failure = Some(Failure::RemoteClosed);
                }
                continue;
            }
            match c.read() {
                Ok(frames) => received.extend(frames.into_iter().map(|f| (c.key, f))),
                Err(e) => c.failure = Some(e),
            }
        }
        let mut invalid = Vec::new();
        for (key, frame) in received {
            if invalid.contains(&key) {
                continue;
            }
            let Some(index) = self.connections.iter().position(|c| c.key == key) else {
                continue;
            };
            let mut c = self.connections.remove(index);
            if let Err(e) = self.frame(&mut c, frame) {
                c.failure = Some(e);
                invalid.push(key);
            }
            self.connections.push(c);
        }
        let now = Instant::now();
        for c in &mut self.connections {
            if c.failure.is_some() {
                continue;
            }
            if matches!(c.phase, Phase::Ready(_)) {
                if now.duration_since(c.received) >= self.config.peer_timeout {
                    c.failure = Some(Failure::Timeout);
                } else if now.duration_since(c.ping) >= self.config.heartbeat_interval {
                    c.control(wire::PING, &[]);
                    c.ping = now;
                }
            } else if now.duration_since(c.born) >= self.config.handshake_timeout {
                c.failure = Some(Failure::Timeout);
            }
        }
        self.cleanup();
        if self.reason.is_some() {
            return;
        }
        // Larger IDs initiate client-to-client links, avoiding duplicate dials.
        if self.role != Role::Host && self.session.is_some() {
            let targets: Vec<_> = self
                .members
                .values()
                .filter(|p| {
                    p.id < self.config.player
                        && Some(p.id) != self.host
                        && !self.has_peer(p.id)
                        && self
                            .retry
                            .get(&p.id)
                            .is_none_or(|time| *time <= Instant::now())
                })
                .cloned()
                .collect();
            for peer in targets {
                if self.connections.len() >= MAX_CONNECTIONS {
                    break;
                }
                match self.dial(peer.address, Phase::Peer(peer.id)) {
                    Ok(mut c) => {
                        c.control(wire::PEER_HELLO, &self.hello());
                        self.connections.push(c);
                    }
                    Err(reason) => {
                        self.retry
                            .insert(peer.id, Instant::now() + Duration::from_millis(500));
                        self.emit(Event::Error {
                            peer: Some(peer.id),
                            reason,
                        });
                    }
                }
            }
        }
    }

    fn send_datagram(&self, peer: PlayerId, bytes: &[u8]) -> Result<(), Failure> {
        let socket = self.udp.as_ref().ok_or(Failure::Protocol("UDP disabled"))?;
        if !self
            .connections
            .iter()
            .any(|c| matches!(c.phase,Phase::Ready(id) if id==peer) && c.failure.is_none())
        {
            return Err(Failure::PeerUnavailable);
        }
        let target = self
            .members
            .get(&peer)
            .ok_or(Failure::PeerUnavailable)?
            .address;
        let mut data = b"UFUD\0\x01\0\0".to_vec();
        data.extend_from_slice(&self.session.ok_or(Failure::PeerUnavailable)?.0);
        data.extend_from_slice(&self.config.player.0);
        data.extend_from_slice(&peer.0);
        data.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        data.extend_from_slice(bytes);
        match socket.send_to(&data, target) {
            Ok(n) if n == data.len() => Ok(()),
            Ok(_) => Err(Failure::Protocol("short UDP send")),
            Err(e) => Err(Failure::Io(e.kind())),
        }
    }

    fn datagrams(&mut self) {
        for _ in 0..32 {
            let Some(socket) = &self.udp else { return };
            let mut buffer = [0u8; super::MAX_UNRELIABLE_MESSAGE + 59];
            let (len, source) = match socket.recv_from(&mut buffer) {
                Ok(pair) => pair,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if cfg!(windows) && e.raw_os_error() == Some(10040) => continue,
                Err(e) => {
                    self.emit(Event::Error {
                        peer: None,
                        reason: Failure::Io(e.kind()),
                    });
                    return;
                }
            };
            if !(58..=super::MAX_UNRELIABLE_MESSAGE + 58).contains(&len)
                || &buffer[..8] != b"UFUD\0\x01\0\0"
            {
                continue;
            }
            if self.session != Some(SessionId(buffer[8..24].try_into().unwrap()))
                || buffer[40..56] != self.config.player.0
                || u16::from_be_bytes(buffer[56..58].try_into().unwrap()) as usize != len - 58
            {
                continue;
            }
            let peer = PlayerId(buffer[24..40].try_into().unwrap());
            if !self
                .connections
                .iter()
                .any(|c| matches!(c.phase,Phase::Ready(id) if id==peer) && c.failure.is_none())
                || self
                    .members
                    .get(&peer)
                    .is_none_or(|p| SocketAddr::V4(p.address) != source)
            {
                continue;
            }
            self.emit(Event::UnreliableData {
                from: peer,
                bytes: buffer[58..len].to_vec(),
            });
        }
    }
}

/// Marks even an unwinding worker as stopped before disconnecting the channel.
struct ExitStatus(Arc<AtomicU8>);
impl Drop for ExitStatus {
    fn drop(&mut self) {
        let _ = self.0.compare_exchange(
            0,
            StopReason::WorkerFailed as u8,
            Ordering::Release,
            Ordering::Relaxed,
        );
    }
}

pub(super) fn run(
    config: Config,
    role: Role,
    commands: mpsc::Receiver<Command>,
    events: mpsc::SyncSender<Event>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
) {
    let _exit = ExitStatus(status.clone());
    let setup = || -> io::Result<(TcpListener, SocketAddrV4)> {
        let listener = TcpListener::bind(config.bind)?;
        listener.set_nonblocking(true)?;
        let SocketAddr::V4(address) = listener.local_addr()? else {
            unreachable!()
        };
        Ok((listener, address))
    };
    let (listener, address) = match setup() {
        Ok(pair) => pair,
        Err(e) => {
            let _ = events.try_send(Event::Error {
                peer: None,
                reason: Failure::Io(e.kind()),
            });
            return;
        }
    };
    let udp = if config.unreliable {
        match UdpSocket::bind(address).and_then(|s| {
            s.set_nonblocking(true)?;
            Ok(s)
        }) {
            Ok(s) => Some(s),
            Err(e) => {
                let _ = events.try_send(Event::Error {
                    peer: None,
                    reason: Failure::Io(e.kind()),
                });
                return;
            }
        }
    } else {
        None
    };
    let mut worker = Worker {
        config,
        role,
        listener,
        address,
        session: None,
        host: None,
        members: BTreeMap::new(),
        connections: Vec::new(),
        retry: BTreeMap::new(),
        next_key: 0,
        events,
        reason: None,
        udp,
        accepting: true,
    };
    worker.emit(Event::Listening(address));
    match role {
        Role::Host => {
            worker.session = Some(SessionId(PlayerId::random().0));
            worker.host = Some(worker.config.player);
            worker.members.insert(
                worker.config.player,
                Peer {
                    id: worker.config.player,
                    address,
                },
            );
            worker.emit(Event::Joined {
                session: worker.session.unwrap(),
                host: worker.config.player,
            });
            worker.announce();
        }
        Role::Client(host) => match worker.dial(host, Phase::Welcome) {
            Ok(mut c) => {
                let mut bytes = worker.config.player.0.to_vec();
                bytes.extend_from_slice(&worker.address.port().to_be_bytes());
                bytes.extend_from_slice(&worker.config.compatibility);
                c.control(wire::JOIN, &bytes);
                worker.connections.push(c);
            }
            Err(reason) => {
                worker.emit(Event::Error { peer: None, reason });
                worker.reason.get_or_insert(StopReason::HostLost);
            }
        },
    }
    while worker.reason.is_none() && !stop.load(Ordering::Acquire) {
        for _ in 0..64 {
            match commands.try_recv() {
                Ok(Command::Send(peer, bytes)) => {
                    let result = worker
                        .connections
                        .iter_mut()
                        .find(|c| {
                            matches!(c.phase, Phase::Ready(id) if id == peer) && c.failure.is_none()
                        })
                        .ok_or(Failure::PeerUnavailable)
                        .and_then(|c| c.queue(wire::DATA, &bytes));
                    if let Err(reason) = result {
                        worker.emit(Event::Error {
                            peer: Some(peer),
                            reason,
                        });
                    }
                }
                Ok(Command::Datagram(peer, bytes)) => {
                    if let Err(reason) = worker.send_datagram(peer, &bytes) {
                        worker.emit(Event::Error {
                            peer: Some(peer),
                            reason,
                        });
                    }
                }
                Ok(Command::Accepting(accepting)) => {
                    if role == Role::Host {
                        worker.accepting = accepting;
                    }
                }
                Err(_) => break,
            }
        }
        worker.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    let reason = worker.reason.unwrap_or(StopReason::Requested);
    status.store(reason as u8, Ordering::Release);
    // Closing sockets signals EOF. Buffered application sends are not promised
    // to drain on explicit leave, overload or a terminal error.
}
