/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Optional LAN discovery and a standalone console, without GameKit access.
mod cli;
mod discovery;
pub use cli::run_cli;
pub use discovery::{Advertisement, Config, Discovery, Event, Room, DEFAULT_PORT};

use std::net::{SocketAddr, SocketAddrV4};

/// Parse numeric IP:port, reserving bracketed IPv6 syntax for future transport.
/// No DNS resolution, broadcast lookup or network I/O occurs here.
pub fn ipv4_endpoint(input: &str) -> Result<SocketAddrV4, String> {
    match input.parse::<SocketAddr>() {
        Ok(SocketAddr::V4(address))
            if address.port() != 0
                && !address.ip().is_unspecified()
                && !address.ip().is_multicast()
                && !address.ip().is_broadcast() =>
        {
            Ok(address)
        }
        Ok(SocketAddr::V6(_)) => {
            Err("IPv6 address parsed, but transport v1 supports IPv4 only".into())
        }
        _ => Err("Enter a numeric IPv4 address and port 1..65535, e.g. 192.168.1.10:42018".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_address_is_numeric_explicit_and_independent_of_discovery() {
        assert_eq!(ipv4_endpoint("10.8.0.2:1234").unwrap().port(), 1234);
        for bad in [
            "hostname:1234",
            "127.0.0.1",
            "127.0.0.1:0",
            "0.0.0.0:1",
            "255.255.255.255:1",
        ] {
            assert!(ipv4_endpoint(bad).is_err());
        }
        assert!(ipv4_endpoint("[::1]:1234").unwrap_err().contains("IPv6"));
    }
}
