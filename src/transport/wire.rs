/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Version 1 wire format: explicit network byte order, never Rust layouts.
use super::{Failure, Peer, PlayerId, MAX_MESSAGE, MAX_PLAYERS};
use std::net::{Ipv4Addr, SocketAddrV4};

pub(super) const HEADER: usize = 20;
pub(super) const JOIN: u8 = 1;
pub(super) const WELCOME: u8 = 2;
pub(super) const ROSTER: u8 = 3;
pub(super) const PEER_HELLO: u8 = 4;
pub(super) const DATA: u8 = 5;
pub(super) const PING: u8 = 6;
pub(super) const PONG: u8 = 7;
pub(super) const REJECT: u8 = 8;

pub(super) struct Frame {
    pub kind: u8,
    pub bytes: Vec<u8>,
}

pub(super) fn encode(kind: u8, sequence: u64, bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(HEADER + bytes.len());
    result.extend_from_slice(b"UFTP");
    result.extend_from_slice(&super::PROTOCOL_VERSION.to_be_bytes());
    result.extend_from_slice(&[kind, 0]);
    result.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    result.extend_from_slice(&sequence.to_be_bytes());
    result.extend_from_slice(bytes);
    result
}

/// Incremental parser. The worker bounds the input buffer before appending.
#[derive(Default)]
pub(super) struct Decoder {
    pub buffer: Vec<u8>,
    sequence: u64,
}
impl Decoder {
    pub fn next(&mut self) -> Result<Option<Frame>, Failure> {
        if self.buffer.len() < HEADER {
            return Ok(None);
        }
        let h = &self.buffer[..HEADER];
        if &h[..4] != b"UFTP" || h[4..6] != super::PROTOCOL_VERSION.to_be_bytes() || h[7] != 0 {
            return Err(Failure::Protocol("bad magic, version or flags"));
        }
        if !(JOIN..=REJECT).contains(&h[6]) {
            return Err(Failure::Protocol("unknown frame kind"));
        }
        let size = u32::from_be_bytes(h[8..12].try_into().unwrap()) as usize;
        if size > MAX_MESSAGE {
            return Err(Failure::Protocol("frame too large"));
        }
        let sequence = u64::from_be_bytes(h[12..20].try_into().unwrap());
        if sequence != self.sequence {
            return Err(Failure::Protocol("sequence mismatch"));
        }
        if self.buffer.len() < HEADER + size {
            return Ok(None);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Failure::Protocol("sequence exhausted"))?;
        let frame = Frame {
            kind: h[6],
            bytes: self.buffer[HEADER..HEADER + size].to_vec(),
        };
        self.buffer.drain(..HEADER + size);
        Ok(Some(frame))
    }
}

pub(super) fn roster(peers: &[Peer]) -> Vec<u8> {
    let mut bytes = vec![peers.len() as u8];
    for peer in peers {
        bytes.extend_from_slice(&peer.id.0);
        bytes.extend_from_slice(&peer.address.ip().octets());
        bytes.extend_from_slice(&peer.address.port().to_be_bytes());
    }
    bytes
}

pub(super) fn parse_roster(bytes: &[u8]) -> Result<Vec<Peer>, Failure> {
    let Some(&count) = bytes.first() else {
        return Err(Failure::Protocol("empty roster"));
    };
    let count = count as usize;
    if !(1..=MAX_PLAYERS).contains(&count) || bytes.len() != 1 + 22 * count {
        return Err(Failure::Protocol("bad roster length"));
    }
    let mut peers: Vec<Peer> = Vec::new();
    for entry in bytes[1..].as_chunks::<22>().0 {
        let id = PlayerId(entry[..16].try_into().unwrap());
        let ip = Ipv4Addr::from(<[u8; 4]>::try_from(&entry[16..20]).unwrap());
        let port = u16::from_be_bytes(entry[20..22].try_into().unwrap());
        if id.0 == [0; 16] || port == 0 || peers.iter().any(|p| p.id == id) {
            return Err(Failure::Protocol("invalid or duplicate roster identity"));
        }
        peers.push(Peer {
            id,
            address: SocketAddrV4::new(ip, port),
        });
    }
    Ok(peers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_and_coalesced_binary_frames_preserve_boundaries() {
        let frames = [
            encode(DATA, 0, &[]),
            encode(DATA, 1, &[0, 255, 0]),
            encode(DATA, 2, &vec![42; MAX_MESSAGE]),
        ]
        .concat();
        let mut decoder = Decoder::default();
        let mut messages = Vec::new();
        for part in frames.chunks(7) {
            decoder.buffer.extend_from_slice(part);
            while let Some(frame) = decoder.next().unwrap() {
                messages.push(frame.bytes);
            }
        }
        assert_eq!(messages, [vec![], vec![0, 255, 0], vec![42; MAX_MESSAGE]]);
        assert!(decoder.buffer.is_empty());
    }

    #[test]
    fn rejects_bad_headers_before_allocating_payload() {
        for (offset, value) in [(0, 0), (5, 2), (6, 99), (7, 1), (8, 255), (19, 1)] {
            let mut bytes = encode(DATA, 0, &[]);
            bytes[offset] = value;
            let mut decoder = Decoder {
                buffer: bytes,
                ..Default::default()
            };
            assert!(decoder.next().is_err(), "offset {offset}");
        }
        let mut decoder = Decoder {
            buffer: encode(DATA, 0, &[1, 2]),
            ..Default::default()
        };
        decoder.buffer.pop();
        assert!(decoder.next().unwrap().is_none());
        decoder.buffer.push(2);
        assert_eq!(decoder.next().unwrap().unwrap().bytes, [1, 2]);
        decoder.buffer = encode(DATA, 0, &[]);
        assert!(decoder.next().is_err());
    }

    #[test]
    fn roster_validates_size_ids_and_ports() {
        let peer = Peer {
            id: PlayerId([1; 16]),
            address: "127.0.0.1:1234".parse().unwrap(),
        };
        assert_eq!(
            parse_roster(&roster(std::slice::from_ref(&peer))).unwrap(),
            std::slice::from_ref(&peer)
        );
        assert!(parse_roster(&roster(&[peer.clone(), peer])).is_err());
        for bytes in [vec![], vec![0], vec![5], vec![1, 2]] {
            assert!(parse_roster(&bytes).is_err());
        }
    }
}
