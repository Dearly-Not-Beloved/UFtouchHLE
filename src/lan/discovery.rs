/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Passive UDP broadcast discovery: publishers need no shared listening port.
use crate::transport::SessionId;
use std::collections::BTreeMap;
use std::io;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 42019;
const MAX_PACKET: usize = 151;
const MAX_ROOMS: usize = 64;
const MAX_ENDPOINTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advertisement {
    /// The actual transport SessionId, not an independently invented room ID.
    pub id: SessionId,
    pub name: String,
    pub port: u16,
    pub players: u8,
    pub capacity: u8,
    pub protocol: u16,
    pub compatibility: [u8; 16],
}
impl Advertisement {
    fn valid(&self) -> bool {
        self.id.0 != [0; 16]
            && !self.name.is_empty()
            && self.name.len() <= 96
            && !self.name.chars().any(char::is_control)
            && self.port != 0
            && self.players > 0
            && self.players <= self.capacity
            && self.capacity <= 4
            && self.protocol > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub info: Advertisement,
    /// Derived from UDP source IP plus advertised TCP port, never a claimed IP.
    /// SocketAddr reserves an IPv6 representation; v1 packets are IPv4 only.
    pub endpoints: Vec<SocketAddr>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddrV4,
    pub targets: Vec<SocketAddrV4>,
    pub interval: Duration,
    pub expires: Duration,
    pub compatibility: Option<[u8; 16]>,
}
impl Config {
    pub fn browser() -> Self {
        Self {
            bind: SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, DEFAULT_PORT),
            targets: vec![],
            interval: Duration::from_secs(1),
            expires: Duration::from_secs(4),
            compatibility: None,
        }
    }
    pub fn publisher() -> Self {
        Self {
            bind: "0.0.0.0:0".parse().unwrap(),
            targets: vec![SocketAddrV4::new(
                std::net::Ipv4Addr::BROADCAST,
                DEFAULT_PORT,
            )],
            ..Self::browser()
        }
    }
    pub(super) fn validate(&self) -> io::Result<()> {
        if self.targets.len() > 16
            || self.targets.iter().any(|p| p.port() == 0)
            || self.interval < Duration::from_millis(50)
            || self.interval > Duration::from_secs(10)
            || self.expires < self.interval * 2
            || self.expires > Duration::from_secs(60)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid discovery limits",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Ready(SocketAddrV4),
    Rooms(Vec<Room>),
    Error(String),
    Stopped(&'static str),
}

/// Separate worker with bounded events and scalar update commands.
pub struct Discovery {
    events: mpsc::Receiver<Event>,
    updates: mpsc::SyncSender<u8>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
    capacity: u8,
    stopped: bool,
}
impl Discovery {
    pub fn browse(config: Config) -> io::Result<Self> {
        Self::start(config, None)
    }
    pub fn advertise(config: Config, room: Advertisement) -> io::Result<Self> {
        if !room.valid() || config.targets.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid room or empty broadcast targets",
            ));
        }
        Self::start(config, Some(room))
    }
    fn start(config: Config, room: Option<Advertisement>) -> io::Result<Self> {
        config.validate()?;
        let capacity = room.as_ref().map_or(0, |r| r.capacity);
        let (events_tx, events) = mpsc::sync_channel(64);
        let (updates, updates_rx) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicU8::new(0));
        let worker_stop = stop.clone();
        let worker_status = status.clone();
        std::thread::Builder::new()
            .name("touchHLE LAN discovery".into())
            .spawn(move || {
                // Even unwinding drops the event sender, yielding Stopped to
                // poll.
                let result = run(
                    config,
                    room,
                    events_tx,
                    updates_rx,
                    worker_stop,
                    worker_status.clone(),
                );
                worker_status.store(result, Ordering::Release);
            })?;
        Ok(Self {
            events,
            updates,
            stop,
            status,
            capacity,
            stopped: false,
        })
    }
    pub fn update_players(&self, players: u8) -> Result<(), &'static str> {
        if players == 0 || players > self.capacity {
            return Err("invalid publisher player count");
        }
        self.updates
            .try_send(players)
            .map_err(|_| "discovery update queue full or stopped")
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
    pub fn poll(&mut self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Disconnected) if !self.stopped => {
                self.stopped = true;
                Some(Event::Stopped(match self.status.load(Ordering::Acquire) {
                    1 => "requested",
                    2 => "event backpressure",
                    3 => "socket error",
                    _ => "worker exited",
                }))
            }
            _ => None,
        }
    }
}
impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop();
    }
}

fn packet(room: &Advertisement, sequence: u64, goodbye: bool) -> Vec<u8> {
    let mut bytes = b"UFLD\x00\x01".to_vec();
    bytes.extend_from_slice(&[if goodbye { 2 } else { 1 }, 0]);
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.extend_from_slice(&room.id.0);
    bytes.extend_from_slice(&room.protocol.to_be_bytes());
    bytes.extend_from_slice(&room.port.to_be_bytes());
    bytes.extend_from_slice(&[room.players, room.capacity]);
    bytes.extend_from_slice(&room.compatibility);
    bytes.push(room.name.len() as u8);
    bytes.extend_from_slice(room.name.as_bytes());
    bytes
}
fn parse(bytes: &[u8]) -> Option<(Advertisement, u64, bool)> {
    if bytes.len() < 55
        || bytes.len() > MAX_PACKET
        || &bytes[..6] != b"UFLD\x00\x01"
        || ![1, 2].contains(&bytes[6])
        || bytes[7] != 0
        || bytes.len() != 55 + bytes[54] as usize
    {
        return None;
    }
    let room = Advertisement {
        id: SessionId(bytes[16..32].try_into().ok()?),
        protocol: u16::from_be_bytes(bytes[32..34].try_into().ok()?),
        port: u16::from_be_bytes(bytes[34..36].try_into().ok()?),
        players: bytes[36],
        capacity: bytes[37],
        compatibility: bytes[38..54].try_into().ok()?,
        name: std::str::from_utf8(&bytes[55..]).ok()?.into(),
    };
    room.valid().then(|| {
        (
            room,
            u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
            bytes[6] == 2,
        )
    })
}

struct Entry {
    info: Advertisement,
    sequence: u64,
    endpoints: BTreeMap<SocketAddr, Instant>,
}
#[derive(Default)]
struct Table(
    BTreeMap<[u8; 16], Entry>,
    BTreeMap<([u8; 16], SocketAddr), (u64, Instant)>,
);
impl Table {
    fn snapshot(&self) -> Vec<Room> {
        self.0
            .values()
            .map(|e| Room {
                info: e.info.clone(),
                endpoints: e.endpoints.keys().copied().collect(),
            })
            .collect()
    }
    fn ingest(
        &mut self,
        room: Advertisement,
        seq: u64,
        bye: bool,
        source: SocketAddrV4,
        now: Instant,
    ) {
        let endpoint = SocketAddr::V4(SocketAddrV4::new(*source.ip(), room.port));
        if source.ip().is_unspecified() || source.ip().is_multicast() || source.ip().is_broadcast()
        {
            return;
        }
        if bye {
            if let Some(entry) = self.0.get_mut(&room.id.0) {
                if seq >= entry.sequence && entry.endpoints.remove(&endpoint).is_some() {
                    if self.1.len() >= MAX_ROOMS * MAX_ENDPOINTS {
                        self.1.pop_first();
                    }
                    self.1.insert((room.id.0, endpoint), (seq, now));
                }
                if entry.endpoints.is_empty() {
                    self.0.remove(&room.id.0);
                }
            }
            return;
        }
        if self
            .1
            .get(&(room.id.0, endpoint))
            .is_some_and(|(old, _)| seq <= *old)
        {
            return;
        }
        if !self.0.contains_key(&room.id.0) && self.0.len() >= MAX_ROOMS {
            return;
        }
        let entry = self.0.entry(room.id.0).or_insert_with(|| Entry {
            info: room.clone(),
            sequence: seq,
            endpoints: BTreeMap::new(),
        });
        if seq < entry.sequence {
            return;
        }
        entry.info = room;
        entry.sequence = seq;
        if entry.endpoints.contains_key(&endpoint) || entry.endpoints.len() < MAX_ENDPOINTS {
            entry.endpoints.insert(endpoint, now);
        }
    }
    fn expire(&mut self, now: Instant, ttl: Duration) {
        self.1
            .retain(|_, (_, seen)| now.duration_since(*seen) < ttl);
        self.0.retain(|_, e| {
            e.endpoints
                .retain(|_, seen| now.duration_since(*seen) < ttl);
            !e.endpoints.is_empty()
        });
    }
}

fn emit(tx: &mpsc::SyncSender<Event>, event: Event) -> bool {
    tx.try_send(event).is_ok()
}
fn run(
    config: Config,
    mut room: Option<Advertisement>,
    tx: mpsc::SyncSender<Event>,
    updates: mpsc::Receiver<u8>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
) -> u8 {
    // Store the status before the channel is disconnected, including failures.
    let setup = || -> io::Result<UdpSocket> {
        let socket = UdpSocket::bind(config.bind)?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(socket)
    };
    let socket = match setup() {
        Ok(s) => s,
        Err(e) => {
            emit(
                &tx,
                Event::Error(format!("Discovery bind {}: {e}", config.bind)),
            );
            status.store(3, Ordering::Release);
            return 3;
        }
    };
    let address = match socket.local_addr() {
        Ok(SocketAddr::V4(a)) => a,
        _ => {
            status.store(3, Ordering::Release);
            return 3;
        }
    };
    if !emit(&tx, Event::Ready(address)) {
        status.store(2, Ordering::Release);
        return 2;
    }
    let mut table = Table::default();
    let mut previous = Vec::new();
    let mut next = Instant::now();
    let mut sequence = 0u64;
    let mut last_error = BTreeMap::new();
    while !stop.load(Ordering::Acquire) {
        for players in updates.try_iter().take(8) {
            if let Some(room) = room.as_mut() {
                room.players = players;
                next = Instant::now();
            }
        }
        if let Some(room) = room.as_ref() {
            if Instant::now() >= next {
                let Some(seq) = sequence.checked_add(1) else {
                    break;
                };
                sequence = seq;
                let bytes = packet(room, sequence, false);
                for target in &config.targets {
                    if let Err(e) = socket.send_to(&bytes, target) {
                        // Deduplicate persistent errors, but let other NICs
                        // work.
                        let error = format!("Discovery broadcast to {target}: {e}");
                        if last_error.get(target) != Some(&e.kind()) {
                            if !emit(&tx, Event::Error(error.clone())) {
                                status.store(2, Ordering::Release);
                                return 2;
                            }
                            last_error.insert(*target, e.kind());
                        }
                    } else {
                        last_error.remove(target);
                    }
                }
                next = Instant::now() + config.interval;
            }
        } else {
            for _ in 0..32 {
                let mut bytes = [0; MAX_PACKET + 1];
                match socket.recv_from(&mut bytes) {
                    Ok((len, SocketAddr::V4(source))) => {
                        if let Some((room, seq, bye)) = parse(&bytes[..len]) {
                            if config
                                .compatibility
                                .is_none_or(|tag| tag == room.compatibility)
                            {
                                table.ingest(room, seq, bye, source, Instant::now());
                            }
                        }
                    }
                    Ok(_) => (),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    // Winsock reports an oversized UDP datagram as WSAEMSGSIZE.
                    Err(e) if cfg!(windows) && e.raw_os_error() == Some(10040) => continue,
                    Err(e) => {
                        emit(&tx, Event::Error(format!("Discovery receive: {e}")));
                        status.store(3, Ordering::Release);
                        return 3;
                    }
                }
            }
            table.expire(Instant::now(), config.expires);
            let rooms = table.snapshot();
            if rooms != previous {
                if !emit(&tx, Event::Rooms(rooms.clone())) {
                    status.store(2, Ordering::Release);
                    return 2;
                }
                previous = rooms;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(room) = room {
        let bytes = packet(&room, sequence.saturating_add(1), true);
        for target in &config.targets {
            let _ = socket.send_to(&bytes, target);
        }
    }
    status.store(1, Ordering::Release);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    fn room() -> Advertisement {
        Advertisement {
            id: SessionId([1; 16]),
            name: "测试 Room".into(),
            port: 42018,
            players: 1,
            capacity: 4,
            protocol: 1,
            compatibility: [7; 16],
        }
    }
    #[test]
    fn datagrams_validate_lengths_versions_utf8_and_fields() {
        let ad = room();
        let bytes = packet(&ad, 7, false);
        assert_eq!(parse(&bytes), Some((ad, 7, false)));
        for len in 0..bytes.len() {
            assert!(parse(&bytes[..len]).is_none());
        }
        for (at, value) in [
            (0, 0),
            (5, 2),
            (6, 3),
            (7, 1),
            (36, 0),
            (37, 5),
            (54, 255),
            (55, 255),
        ] {
            let mut bad = bytes.clone();
            bad[at] = value;
            assert!(parse(&bad).is_none());
        }
    }
    #[test]
    fn table_deduplicates_updates_multiple_interfaces_and_expires() {
        let mut table = Table::default();
        let now = Instant::now();
        let source = "10.0.0.2:123".parse().unwrap();
        table.ingest(room(), 1, false, source, now);
        table.ingest(room(), 1, false, source, now);
        table.ingest(room(), 1, false, "10.1.0.2:124".parse().unwrap(), now);
        assert_eq!(table.snapshot().len(), 1);
        assert_eq!(table.snapshot()[0].endpoints.len(), 2);
        let mut update = room();
        update.players = 3;
        table.ingest(update, 2, false, source, now);
        table.ingest(room(), 1, false, source, now);
        assert_eq!(table.snapshot()[0].info.players, 3);
        table.expire(now + Duration::from_secs(5), Duration::from_secs(4));
        assert!(table.snapshot().is_empty());
    }
    #[test]
    fn goodbye_removes_only_known_endpoint_and_table_is_bounded() {
        let mut table = Table::default();
        let now = Instant::now();
        let source = "127.0.0.1:1".parse().unwrap();
        for n in 1..=65 {
            let mut ad = room();
            ad.id.0 = [n; 16];
            table.ingest(ad, 1, false, source, now);
        }
        assert_eq!(table.snapshot().len(), 64);
        table.ingest(room(), 2, true, "127.0.0.2:2".parse().unwrap(), now);
        assert_eq!(table.snapshot().len(), 64);
        table.ingest(room(), 2, true, source, now);
        assert_eq!(table.snapshot().len(), 63);
        table.ingest(room(), 1, false, source, now);
        assert_eq!(table.snapshot().len(), 63);
    }
}
