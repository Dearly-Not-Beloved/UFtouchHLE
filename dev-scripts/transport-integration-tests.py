#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Independent process tests; only 127.0.0.1 ephemeral ports, no game assets.

Build first: CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release --example transport_peer
Run: python3 dev-scripts/transport-integration-tests.py
Raw JSON event logs are saved under target/transport-tests/.
"""
import json
import os
import hashlib
from pathlib import Path
import queue
import socket
import struct
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/transport-tests"
BINARY = Path(os.environ.get("UF_TRANSPORT_TEST_BINARY", ROOT / "target/release/examples/transport_peer"))
NODES = []


def identity(n):
    return f"{n:032x}"


class Node:
    def __init__(self, label, number, host=None):
        self.id = identity(number)
        self.events = queue.Queue()
        self.saved = []
        self.process = subprocess.Popen(
            [str(BINARY), "client" if host else "host", "127.0.0.1:0",
             host or "-", self.id, "--fast"], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        NODES.append(self)
        def read():
            with (OUT / (label + ".jsonl")).open("w") as log:
                for line in self.process.stdout:
                    log.write(line)
                    log.flush()
                    try:
                        self.events.put(json.loads(line))
                    except ValueError:
                        self.events.put({"event": "invalid_json", "line": line})
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.address = self.wait("listening")["address"]

    def wait(self, event, predicate=lambda value: True, timeout=6):
        for index, value in enumerate(self.saved):
            if value["event"] == event and predicate(value):
                return self.saved.pop(index)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                value = self.events.get(timeout=max(0.001, deadline - time.monotonic()))
            except queue.Empty:
                break
            if value["event"] == event and predicate(value):
                return value
            self.saved.append(value)
        raise AssertionError(f"pid={self.process.pid} missing {event}, exit={self.process.poll()}, recent={self.saved[-6:]}")

    def command(self, **value):
        self.process.stdin.write(json.dumps(value) + "\n")
        self.process.stdin.flush()
        return self.wait("command_result")["error"]

    def send(self, peer, data):
        assert self.command(command="send", to=peer.id, bytes=data) is None

    def data(self, peer, data):
        received = self.wait("data")
        assert received["from"] == peer.id and received["bytes"] == data

    def leave(self):
        assert self.command(command="leave") is None
        assert self.wait("stopped")["reason"] == "Requested"
        assert self.process.wait(timeout=3) == 0


def raw_connection(host):
    ip, port = host.address.split(":")
    stream = socket.create_connection((ip, int(port)), timeout=2)
    stream.settimeout(2)
    return stream


def frame(kind, sequence, payload, version=1):
    return struct.pack("!4sHBBIQ", b"UFTP", version, kind, 0, len(payload), sequence) + payload


def join(number, compatibility=b"UF-TRANSPORT-V1!"):
    return bytes.fromhex(identity(number)) + struct.pack("!H", 12345) + compatibility


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    paths = [BINARY, *sorted((ROOT / "src/transport").glob("*.rs"))]
    (OUT / "manifest.json").write_text(json.dumps({
        "time_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": sys.platform,
        "sha256": {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
    }, indent=2) + "\n")
    host = Node("host", 1)
    host.wait("joined")
    collision = subprocess.run([str(BINARY), "host", host.address, "-", identity(99)],
                               input="", capture_output=True, text=True, timeout=6)
    collision_events = [json.loads(line) for line in collision.stdout.splitlines()]
    assert any(e["event"] == "error" and "AddrInUse" in e["reason"] for e in collision_events)
    client = Node("client", 2, host.address)
    host.wait("connected", lambda e: e["peer"] == client.id)
    client.wait("connected", lambda e: e["peer"] == host.id)
    print("PASS independent Host/Client processes connected")

    for payload in ([], [0,255,0,128], list(range(256)) * 256):
        client.send(host, payload); host.data(client, payload)
        host.send(client, payload); client.data(host, payload)
    for batch in range(4):
        messages = [[0,255,batch,index,0] for index in range(20)]
        for message in messages:
            client.send(host, message); host.send(client, message[::-1])
        for message in messages:
            host.data(client, message); client.data(host, message[::-1])
    assert client.command(command="send", to=host.id, bytes=[1], unreliable=True) == "UnsupportedDelivery"
    assert client.command(command="send", to=host.id, bytes=[0] * 65537) == "TooLarge"
    print("PASS bidirectional binary boundaries (0..65536 bytes), 80 ordered messages each way, explicit unsupported mode")

    client.leave()
    host.wait("disconnected", lambda e: e["peer"] == client.id)
    client = Node("client-rejoin", 2, host.address)
    host.wait("connected", lambda e: e["peer"] == client.id)
    client.wait("connected", lambda e: e["peer"] == host.id)
    client.process.kill(); client.process.wait(timeout=3)
    host.wait("disconnected", lambda e: e["peer"] == client.id)
    client = Node("client-recovery", 2, host.address)
    host.wait("connected", lambda e: e["peer"] == client.id)
    client.wait("connected", lambda e: e["peer"] == host.id)
    client.send(host, [7,0,9]); host.data(client, [7,0,9])
    print("PASS graceful leave, process kill, identity reuse and fresh-connection recovery")

    assert client.command(command="pause_events", ms=2000) is None
    for _ in range(100):
        host.send(client, [])
    assert client.wait("stopped")["reason"] == "EventBackpressure"
    assert client.process.wait(timeout=3) == 0
    host.wait("disconnected", lambda e: e["peer"] == client.id)
    client = Node("client-overflow-recovery", 2, host.address)
    host.wait("connected", lambda e: e["peer"] == client.id)
    client.wait("connected", lambda e: e["peer"] == host.id)
    print("PASS stalled event consumer terminates explicitly with bounded backpressure, then rejoins")

    with raw_connection(host) as stream:
        stream.sendall(frame(1, 0, join(9), version=99))
        host.wait("error", lambda e: "version" in e["reason"])
    with raw_connection(host) as stream:
        stream.sendall(frame(1, 0, join(9)))
        host.wait("connected", lambda e: e["peer"] == identity(9))
        host.wait("disconnected", lambda e: e["peer"] == identity(9) and e["reason"] == "Timeout")
    with raw_connection(host) as stream:
        stream.sendall(frame(1, 0, join(9)))
        host.wait("connected", lambda e: e["peer"] == identity(9))
        stream.sendall(frame(5, 0, b"bad sequence"))
        host.wait("disconnected", lambda e: e["peer"] == identity(9) and "sequence" in e["reason"])
    with raw_connection(host) as stream:
        stream.sendall(b"U")
        host.wait("error", lambda e: e["reason"] == "Timeout")
    with raw_connection(host) as stream:
        stream.sendall(frame(1, 0, join(9, b"wrong-tag-123456")))
        reply = b""
        while len(reply) < 21:
            part = stream.recv(21 - len(reply))
            assert part, "rejection frame truncated"
            reply += part
        assert reply[:4] == b"UFTP" and reply[-1:] == b"\x03"
    print("PASS malformed version, handshake/heartbeat timeout, bad sequence and incompatible tag; Host survives")

    third = Node("third", 3, host.address)
    fourth = Node("fourth", 4, host.address)
    peers = [host, client, third, fourth]
    for node in peers:
        for other in peers:
            if node is other or {node.id, other.id} == {host.id, client.id}:
                continue
            node.wait("connected", lambda e, other=other: e["peer"] == other.id)
    third.send(fourth, [3,0,4]); fourth.data(third, [3,0,4])
    fourth.send(client, [4,255,2]); client.data(fourth, [4,255,2])
    fifth = Node("fifth-rejected", 5, host.address)
    fifth.wait("error", lambda e: e["reason"] == "RoomFull")
    fifth.wait("stopped")
    assert fifth.process.wait(timeout=3) == 0
    duplicate = Node("duplicate-rejected", 2, host.address)
    duplicate.wait("error", lambda e: e["reason"] == "DuplicateIdentity")
    duplicate.wait("stopped")
    assert duplicate.process.wait(timeout=3) == 0
    print("PASS four-player direct mesh, client-to-client data, fifth/duplicate admission refused")

    third.process.kill(); third.process.wait(timeout=3)
    for node in [host, client, fourth]:
        node.wait("disconnected", lambda e: e["peer"] == third.id)
    third = Node("third-recovery", 3, host.address)
    for node in [host, client, fourth]:
        node.wait("connected", lambda e: e["peer"] == third.id)
        third.wait("connected", lambda e, node=node: e["peer"] == node.id)
    third.send(fourth, [3,0,3]); fourth.data(third, [3,0,3])
    print("PASS four-player membership removal and direct mesh rebuild after process kill")

    host.leave()
    for node in [client, third, fourth]:
        node.wait("disconnected", lambda e: e["peer"] == host.id)
        assert node.wait("stopped")["reason"] == "HostLost"
        assert node.process.wait(timeout=3) == 0
    print("PASS Host loss terminates clients without hidden migration or relay")
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        refused = Node("refused", 11, "127.0.0.1:" + str(reserved.getsockname()[1]))
        # On macOS a bound-but-not-listening port may time out instead of RST.
        refused.wait("error", lambda e: "ConnectionRefused" in e["reason"] or e["reason"] == "Timeout")
        assert refused.wait("stopped")["reason"] == "HostLost"
        assert refused.process.wait(timeout=3) == 0
    print("PASS bind collision and refused connection reported asynchronously")
    print("TRANSPORT PROCESS TESTS PASS (loopback; no GameKit/P2G/physical LAN validation)")


if __name__ == "__main__":
    try:
        main()
    finally:
        for node in NODES:
            if node.process.poll() is None:
                node.process.kill()
            node.process.wait(timeout=3)
            node.reader.join(timeout=2)
            (OUT / f"stderr-{node.process.pid}.log").write_text(node.process.stderr.read())
