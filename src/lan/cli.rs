/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Desktop console entry. The emulator and GameKit are never started here.
use super::{ipv4_endpoint, Advertisement, Config, Discovery, Event as DiscoveryEvent, Room};
use crate::transport::{
    self, Delivery, Event, Failure, PlayerId, Role, SessionId, StopReason, Transport,
};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::net::SocketAddrV4;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TAG: [u8; 16] = *b"UF-TRANSPORT-V1!";
const HELP: &str = "\
Standalone LAN transport console (no GameKit/game integration):
  touchHLE --multiplayer host --name=Room --allow-network-access
  touchHLE --multiplayer browse --allow-network-access
  touchHLE --multiplayer join --address=192.168.1.10:42018 --allow-network-access
Options:
  --bind=IP:PORT              TCP bind (Host 0.0.0.0:42018; Client 0.0.0.0:0)
  --discovery-bind=IP:PORT    Browser UDP bind (0.0.0.0:42019)
  --announce-bind=IP:PORT     Publisher source bind (0.0.0.0:0)
  --broadcast=IP:PORT         Repeat for directed broadcasts on multiple NICs
  --player-id=UUID            Optional stable local identity
  --interval-ms=1000 --ttl-ms=4000
Commands on stdin: list | join ROOM_UUID | join IP:PORT | leave | quit
JSON commands: {\"command\":\"send\",\"to\":\"UUID\",\"bytes\":[0,255,0]}
Output is JSON lines. Manual join never starts UDP discovery.
";

struct Options {
    mode: String,
    name: String,
    bind: SocketAddrV4,
    address: Option<SocketAddrV4>,
    browser: Config,
    publisher: Config,
    player: PlayerId,
}
impl Options {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mode = args.first().ok_or(HELP)?.clone();
        if !["host", "browse", "join"].contains(&mode.as_str()) {
            return Err(HELP.into());
        }
        let mut result = Self {
            bind: if mode == "host" {
                "0.0.0.0:42018"
            } else {
                "0.0.0.0:0"
            }
            .parse()
            .unwrap(),
            mode,
            name: "touchHLE Room".into(),
            address: None,
            browser: Config::browser(),
            publisher: Config::publisher(),
            player: PlayerId::random(),
        };
        result.browser.compatibility = Some(TAG);
        let mut allowed = false;
        let mut targets = Vec::new();
        for arg in &args[1..] {
            if arg == "--allow-network-access" {
                allowed = true;
                continue;
            }
            let (key, value) = arg
                .split_once('=')
                .ok_or_else(|| format!("Unknown option {arg}; use --help"))?;
            match key {
                "--name" => result.name = value.into(),
                "--bind" => result.bind = value.parse().map_err(|_| "--bind needs IPv4:port")?,
                "--address" => result.address = Some(ipv4_endpoint(value)?),
                "--discovery-bind" => {
                    result.browser.bind = value
                        .parse()
                        .map_err(|_| "--discovery-bind needs IPv4:port")?
                }
                "--announce-bind" => {
                    result.publisher.bind = value
                        .parse()
                        .map_err(|_| "--announce-bind needs IPv4:port")?
                }
                "--broadcast" => {
                    targets.push(value.parse().map_err(|_| "--broadcast needs IPv4:port")?)
                }
                "--player-id" => result.player = player(value)?,
                "--interval-ms" | "--ttl-ms" => {
                    let ms = value
                        .parse::<u64>()
                        .map_err(|_| "invalid discovery milliseconds")?;
                    if key == "--interval-ms" {
                        result.browser.interval = Duration::from_millis(ms);
                        result.publisher.interval = Duration::from_millis(ms);
                    } else {
                        result.browser.expires = Duration::from_millis(ms);
                        result.publisher.expires = Duration::from_millis(ms);
                    }
                }
                _ => return Err(format!("Unknown option {key}")),
            }
        }
        if !allowed {
            return Err(
                "LAN networking is disabled. Add --allow-network-access to explicitly enable it"
                    .into(),
            );
        }
        if result.mode == "join" && result.address.is_none() {
            return Err("join requires --address=IP:PORT".into());
        }
        if result.mode != "join" && result.address.is_some() {
            return Err("--address is only for manual join".into());
        }
        if !targets.is_empty() {
            result.publisher.targets = targets;
        }
        if result.mode != "join" {
            result.browser.validate().map_err(|e| e.to_string())?;
            result.publisher.validate().map_err(|e| e.to_string())?;
        }
        // Validate even before any TCP listener is started.
        if result.name.is_empty()
            || result.name.len() > 96
            || result.name.chars().any(char::is_control)
        {
            return Err(
                "Room name must contain 1..96 UTF-8 bytes without control characters".into(),
            );
        }
        Ok(result)
    }
}
fn player(value: &str) -> Result<PlayerId, String> {
    uuid::Uuid::parse_str(value)
        .map(|v| PlayerId(*v.as_bytes()))
        .map_err(|e| format!("Invalid UUID: {e}"))
}
fn id(value: [u8; 16]) -> String {
    uuid::Uuid::from_bytes(value).simple().to_string()
}
fn print(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}
fn rooms_json(rooms: &[Room]) -> Value {
    json!({"event":"rooms","rooms":rooms.iter().map(|r| json!({
        "id":id(r.info.id.0),"name":r.info.name,"endpoints":r.endpoints.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "ip":r.endpoints.first().map(|p| p.ip().to_string()),"port":r.info.port,
        "players":r.info.players,"capacity":r.info.capacity,"protocol":r.info.protocol,
        "joinable":r.info.protocol==transport::PROTOCOL_VERSION && r.info.players<r.info.capacity
    })).collect::<Vec<_>>()})
}
fn failure(reason: &Failure) -> String {
    match reason {
        Failure::Io(io::ErrorKind::ConnectionRefused) => "Connection refused: check Host address, TCP port and listener".into(),
        Failure::Io(io::ErrorKind::AddrInUse) => "Port already in use: choose another --bind port".into(),
        Failure::Timeout => "Connection/peer timed out: check routing, firewall, Wi-Fi isolation and Host availability".into(),
        Failure::RoomFull => "Room is full (Host plus at most three clients)".into(),
        Failure::DuplicateIdentity => "Player ID already connected; leave the old instance or use a different ID".into(),
        Failure::Incompatible => "Host uses an incompatible transport/application tag".into(),
        _ => format!("{reason:?}"),
    }
}
fn connect(options: &Options, address: SocketAddrV4) -> Result<Transport, String> {
    Transport::start(
        transport::Config::new(options.player, options.bind, TAG),
        Role::Client(address),
    )
    .map_err(|e| format!("Cannot start connection: {e}"))
}
fn command(line: &str) -> Result<Value, String> {
    if line.trim_start().starts_with('{') {
        return serde_json::from_str(line).map_err(|e| e.to_string());
    }
    let parts: Vec<_> = line.split_whitespace().collect();
    match parts.as_slice() {
        ["list" | "leave" | "quit"] => Ok(json!({"command":parts[0]})),
        ["join", target] if target.contains(':') => {
            Ok(json!({"command":"join_ip","address":target}))
        }
        ["join", target] => Ok(json!({"command":"join","room":target})),
        _ => Err("Commands: list | join ROOM_UUID | join IP:PORT | leave | quit".into()),
    }
}

/// Standalone CLI; requires explicit network permission and never starts a game.
pub fn run_cli(args: impl Iterator<Item = String>) -> Result<(), String> {
    let args: Vec<_> = args.collect();
    if args.is_empty() || args.iter().any(|s| s == "--help") {
        println!("{HELP}");
        return Ok(());
    }
    let options = Options::parse(args)?;
    let mut discovery = if options.mode == "browse" {
        Some(Discovery::browse(options.browser.clone()).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let mut node = match options.mode.as_str() {
        "host" => Some(
            Transport::start(
                transport::Config::new(options.player, options.bind, TAG),
                Role::Host,
            )
            .map_err(|e| e.to_string())?,
        ),
        "join" => Some(connect(&options, options.address.unwrap())?),
        _ => None,
    };
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    let mut rooms = Vec::new();
    let mut listening = None;
    let mut session = None;
    let mut expected: Option<SessionId> = None;
    let mut joined = false;
    let mut last_error = None;
    let mut publisher_started = false;
    let mut closing: Option<Instant> = None;
    loop {
        for _ in 0..16 {
            let line = match rx.try_recv() {
                Ok(line) => line,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => "quit".into(),
            };
            let result = (|| -> Result<(), String> {
                let value = command(&line)?;
                match value["command"].as_str().ok_or("missing command")? {
                    "list" => print(rooms_json(&rooms)),
                    "quit" => {
                        if let Some(n) = &node {
                            n.disconnect()
                        };
                        if let Some(d) = &discovery {
                            d.stop()
                        };
                        closing.get_or_insert(Instant::now());
                    }
                    "leave" => {
                        if let Some(n) = &node {
                            n.disconnect()
                        };
                        if options.mode == "host" {
                            if let Some(d) = &discovery {
                                d.stop()
                            };
                            closing.get_or_insert(Instant::now());
                        }
                    }
                    "join" | "join_ip" => {
                        if options.mode == "host" || node.is_some() || closing.is_some() {
                            return Err(
                                "Leave the active connection before joining another room".into()
                            );
                        }
                        let address = if value["command"] == "join_ip" {
                            expected = None;
                            ipv4_endpoint(value["address"].as_str().ok_or("missing address")?)?
                        } else {
                            let room_id =
                                player(value["room"].as_str().ok_or("missing room ID")?)?.0;
                            let room = rooms
                                .iter()
                                .find(|r: &&Room| r.info.id.0 == room_id)
                                .ok_or(
                                    "Room expired or not discovered; refresh list or use manual IP",
                                )?;
                            if room.info.protocol != transport::PROTOCOL_VERSION {
                                return Err("Unsupported room transport protocol".into());
                            }
                            if room.info.players >= room.info.capacity {
                                return Err("Room is full".into());
                            }
                            expected = Some(room.info.id);
                            ipv4_endpoint(
                                &room
                                    .endpoints
                                    .first()
                                    .ok_or("Room has no endpoint")?
                                    .to_string(),
                            )?
                        };
                        node = Some(connect(&options, address)?);
                        joined = false;
                        last_error = None;
                    }
                    "send" => {
                        if !joined {
                            return Err("Wait for a validated joined event before sending".into());
                        }
                        let to = player(value["to"].as_str().ok_or("missing target ID")?)?;
                        let bytes = value["bytes"]
                            .as_array()
                            .ok_or("missing bytes")?
                            .iter()
                            .map(|v| {
                                v.as_u64()
                                    .and_then(|v| u8::try_from(v).ok())
                                    .ok_or("invalid byte")
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        node.as_ref()
                            .ok_or("Not connected")?
                            .try_send(to, bytes, Delivery::ReliableOrdered)
                            .map_err(|e| format!("{e:?}"))?;
                    }
                    _ => return Err("unknown command".into()),
                }
                Ok(())
            })();
            print(json!({"event":"command_result","error":result.err()}));
            if closing.is_some() {
                break;
            }
        }
        let mut transport_stopped = false;
        if let Some(n) = &mut node {
            for _ in 0..64 {
                let Some(event) = n.poll() else { break };
                match event {
                    Event::Listening(address) => {
                        listening = Some(address);
                        print(
                            json!({"event":"listening","address":address.to_string(),"player":id(options.player.0)}),
                        );
                    }
                    Event::Joined {
                        session: room_id,
                        host,
                    } => {
                        if expected.is_some_and(|wanted| wanted != room_id) {
                            last_error = Some(
                                "Room changed/restarted; refresh the discovery list".to_string(),
                            );
                            n.disconnect();
                            print(json!({"event":"error","message":last_error}));
                        } else {
                            joined = true;
                            session = Some(room_id);
                            print(
                                json!({"event":"joined","session":id(room_id.0),"host":id(host.0)}),
                            );
                        }
                    }
                    Event::Roster(peers) => {
                        if options.mode == "host" {
                            if let Some(d) = &discovery {
                                if let Err(e) = d.update_players(peers.len() as u8) {
                                    print(json!({"event":"error","message":e}));
                                }
                            }
                        }
                        print(
                            json!({"event":"roster","players":peers.len(),"peers":peers.iter().map(|p|id(p.id.0)).collect::<Vec<_>>()}),
                        );
                    }
                    Event::PeerConnected(peer) if joined => {
                        print(json!({"event":"connected","peer":id(peer.0)}))
                    }
                    Event::PeerConnected(_) => (),
                    Event::Data { from, bytes } | Event::UnreliableData { from, bytes }
                        if joined =>
                    {
                        print(json!({"event":"data","from":id(from.0),"bytes":bytes}))
                    }
                    Event::Data { .. } | Event::UnreliableData { .. } => (),
                    Event::PeerDisconnected { peer, reason } => print(
                        json!({"event":"disconnected","peer":id(peer.0),"message":failure(&reason)}),
                    ),
                    Event::Error { peer, reason } => {
                        let message = failure(&reason);
                        last_error = Some(message.clone());
                        print(
                            json!({"event":"error","peer":peer.map(|p|id(p.0)),"reason":format!("{reason:?}"),"message":message}),
                        );
                    }
                    Event::Stopped(reason) => {
                        print(json!({"event":"transport_stopped","reason":format!("{reason:?}")}));
                        transport_stopped = true;
                        joined = false;
                        if options.mode != "browse" && closing.is_none() {
                            if let Some(d) = &discovery {
                                d.stop()
                            };
                            if reason != StopReason::Requested {
                                return Err(last_error
                                    .clone()
                                    .unwrap_or_else(|| format!("Transport stopped: {reason:?}")));
                            }
                            closing = Some(Instant::now());
                        }
                    }
                }
            }
        }
        if transport_stopped {
            node = None;
        }
        if options.mode == "host" && !publisher_started && node.is_some() && closing.is_none() {
            if let (Some(address), Some(room_id)) = (listening, session) {
                discovery = Some(
                    Discovery::advertise(
                        options.publisher.clone(),
                        Advertisement {
                            id: room_id,
                            name: options.name.clone(),
                            port: address.port(),
                            players: 1,
                            capacity: 4,
                            protocol: transport::PROTOCOL_VERSION,
                            compatibility: TAG,
                        },
                    )
                    .map_err(|e| e.to_string())?,
                );
                publisher_started = true;
            }
        }
        let mut discovery_stopped = false;
        if let Some(d) = &mut discovery {
            for _ in 0..64 {
                let Some(event) = d.poll() else { break };
                match event {
                    DiscoveryEvent::Ready(address) => {
                        print(
                            json!({"event":"discovery_ready","address":address.to_string(),"mode":options.mode}),
                        );
                        if options.mode == "browse" {
                            print(rooms_json(&rooms));
                        }
                    }
                    DiscoveryEvent::Rooms(updated) => {
                        rooms = updated;
                        print(rooms_json(&rooms));
                    }
                    DiscoveryEvent::Error(message) => {
                        print(json!({"event":"discovery_error","message":message}))
                    }
                    DiscoveryEvent::Stopped(reason) => {
                        discovery_stopped = true;
                        print(json!({"event":"discovery_stopped","reason":reason}));
                    }
                }
            }
        }
        if discovery_stopped {
            discovery = None;
            if options.mode == "browse" {
                return if closing.is_some() {
                    Ok(())
                } else {
                    Err("Discovery stopped: check UDP port conflicts/firewall; manual IP mode remains available".into())
                };
            }
        }
        if let Some(start) = closing {
            if (node.is_none() && discovery.is_none()) || start.elapsed() > Duration::from_secs(2) {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_requires_permission_and_manual_join_needs_valid_address() {
        assert!(Options::parse(vec!["host".into()])
            .err()
            .unwrap()
            .contains("disabled"));
        assert!(Options::parse(vec!["join".into(), "--allow-network-access".into()]).is_err());
        let args = ["join", "--allow-network-access", "--address=10.8.0.2:1234"];
        assert!(Options::parse(args.into_iter().map(str::to_string).collect()).is_ok());
        assert_eq!(command("join 10.8.0.2:1234").unwrap()["command"], "join_ip");
    }
}
