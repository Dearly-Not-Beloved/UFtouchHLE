#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""macOS ARM guest TCP/UDP loopback verification for NETWORK_ANALYSIS.md.

Uses existing APIs only; no game assets, discovery, GameKit replacement, or
external connections. All host sockets bind to 127.0.0.1 on ephemeral ports.
Build touchHLE first. Generates the fixture and link stubs under target/.
Usage: python3 dev-scripts/network-analysis-smoke.py
"""

import importlib.util
import os
from pathlib import Path
import plistlib
import socket
import subprocess
import sys
import threading

ROOT = Path(__file__).resolve().parents[1]

GUEST = r'''
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
#define CHECK(x) do { if (!(x)) { printf("FAIL line %d errno %d\n", __LINE__, errno); return 1; } } while (0)
static int readable(int fd) {
    for (int i = 0; i < 2000; ++i) {
        fd_set readset; memset(&readset, 0, sizeof(readset));
        if (fd < 0 || fd >= 1024) return 0;
        ((unsigned int *)&readset)[(unsigned int)fd >> 5] |= 1U << (fd & 31);
        struct timeval timeout = {0, 0};
        int count = select(fd + 1, &readset, NULL, NULL, &timeout);
        if (count < 0) return 0;
        if (count > 0 && (((unsigned int *)&readset)[(unsigned int)fd >> 5] & (1U << (fd & 31)))) return 1;
        usleep(1000);
    }
    return 0;
}
int main(int argc, char **argv) {
    CHECK(argc == 4);
    if (strcmp(argv[1], "disabled") == 0) {
        CHECK(socket(AF_INET, SOCK_DGRAM, 0) == -1);
        CHECK(errno == EPROTONOSUPPORT);
        puts("NETWORK GATE PASS"); return 0;
    }
    const char payload[] = "network!";
    struct sockaddr_in local = {0}, remote = {0};
    local.sin_len = sizeof(local); local.sin_family = AF_INET;
    local.sin_addr.s_addr = inet_addr("127.0.0.1");
    remote = local; remote.sin_port = htons(atoi(argv[2]));
    int udp = socket(AF_INET, SOCK_DGRAM, 0); CHECK(udp >= 0);
    CHECK(bind(udp, (struct sockaddr *)&local, sizeof(local)) == 0);
    socklen_t size = sizeof(local);
    CHECK(getsockname(udp, (struct sockaddr *)&local, &size) == 0 && local.sin_port != 0);
    CHECK(sendto(udp, payload, 8, 0, (struct sockaddr *)&remote, sizeof(remote)) == 8);
    CHECK(readable(udp));
    char buffer[16] = {0}; size = sizeof(remote);
    CHECK(recvfrom(udp, buffer, sizeof(buffer), 0, (struct sockaddr *)&remote, &size) == 8);
    CHECK(memcmp(buffer, payload, 8) == 0); CHECK(close(udp) == 0);
    puts("GUEST UDP LOOPBACK PASS");
    int tcp = socket(AF_INET, SOCK_STREAM, 0); CHECK(tcp >= 0);
    remote.sin_port = htons(atoi(argv[3]));
    CHECK(connect(tcp, (struct sockaddr *)&remote, sizeof(remote)) == 0);
    int sent = 0;
    while (sent < 8) {
        int count = send(tcp, payload + sent, 8 - sent, 0); CHECK(count > 0); sent += count;
    }
    int received = 0;
    while (received < 8) {
        CHECK(readable(tcp));
        int count = recv(tcp, buffer + received, 8 - received, 0); CHECK(count > 0); received += count;
    }
    CHECK(memcmp(buffer, payload, 8) == 0); CHECK(close(tcp) == 0);
    puts("GUEST TCP LOOPBACK PASS"); return 0;
}
'''


def main():
    if sys.platform != "darwin":
        sys.exit("This fixture requires the macOS ARMv7 build toolchain.")
    os.chdir(ROOT)
    spec = importlib.util.spec_from_file_location("integration", ROOT / "dev-scripts/macos-integration-tests.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    out = ROOT / "target/network-analysis/socket-smoke"
    out.mkdir(parents=True, exist_ok=True)
    helper.OUT = str(out)
    helper.STUBS = str(out / "stubs")
    link_args = helper.make_stubs()
    source = out / "socket.c"
    source.write_text(GUEST)
    (out / "start.s").write_text(helper.START_S)
    app = out / "SocketSmoke.app"
    app.mkdir(exist_ok=True)
    with (app / "Info.plist").open("wb") as file:
        plistlib.dump(dict(CFBundleExecutable="SocketSmoke", CFBundleIdentifier="org.touchhle.analysis.socket",
                          CFBundleName="SocketSmoke", CFBundleVersion="1.0"), file)
    helper.run(["clang", *helper.CFLAGS, "-c", str(out / "start.s"), "-o", str(out / "start.o")])
    helper.run(["clang", *helper.CFLAGS, "-c", str(source), "-o", str(out / "socket.o")])
    helper.run(["clang", *helper.CFLAGS, "-nostartfiles", "-e", "start", str(out / "start.o"),
                str(out / "socket.o"), *link_args, "-o", str(app / "SocketSmoke")])
    failures = []

    def echo_udp(server):
        try:
            data, peer = server.recvfrom(64)
            server.sendto(data, peer)
        except OSError as error:
            failures.append(str(error))

    def echo_tcp(server):
        try:
            connection, _ = server.accept()
            with connection:
                connection.settimeout(10)
                data = b""
                while len(data) < 8:
                    chunk = connection.recv(8 - len(data))
                    if not chunk:
                        raise OSError("Unexpected EOF")
                    data += chunk
                connection.sendall(data)
        except OSError as error:
            failures.append(str(error))

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp, socket.socket() as tcp:
        udp.bind(("127.0.0.1", 0)); udp.settimeout(10)
        tcp.bind(("127.0.0.1", 0)); tcp.listen(1); tcp.settimeout(10)
        ports = [str(udp.getsockname()[1]), str(tcp.getsockname()[1])]
        for mode in ("disabled", "enabled"):
            threads = []
            if mode == "enabled":
                threads = [threading.Thread(target=echo_udp, args=(udp,)), threading.Thread(target=echo_tcp, args=(tcp,))]
                for thread in threads:
                    thread.start()
            command = [helper.TOUCHHLE, str(app), "--headless", "--no-error-popup"]
            if mode == "enabled":
                command.append("--allow-network-access")
            result = subprocess.run(command + ["--args", mode, *ports], capture_output=True, text=True, timeout=30)
            (out / (mode + ".log")).write_text(result.stdout + result.stderr)
            for thread in threads:
                thread.join(timeout=12)
            expected = ["NETWORK GATE PASS"] if mode == "disabled" else ["GUEST UDP LOOPBACK PASS", "GUEST TCP LOOPBACK PASS"]
            if result.returncode or any(marker not in result.stdout + result.stderr for marker in expected) or failures:
                print(result.stdout + result.stderr)
                raise RuntimeError("Socket fixture failed: " + str(failures))
            print("; ".join(expected))
    print("Only existing socket happy paths and the network gate were tested; no LAN or GameKit multiplayer was validated.")


if __name__ == "__main__":
    main()
