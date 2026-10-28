#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Summarize actual NetworkTrace logs without inferring visible game success.

Usage: python3 dev-scripts/analyze-mhp2g-lobby-log.py HOST_LOG CLIENT_LOG
Only reads local files. No game execution, network requests, or payload dumps.
"""
import argparse
from collections import Counter
import json
from pathlib import Path


def analyze(lines):
    counts = Counter()
    errors, requests, states, panics = [], [], [], []
    last_call = None
    authenticated = False
    match_installed = False
    truncated = False
    malformed = 0
    for line in lines:
        if "panicked at" in line or line.startswith("Panic at"):
            panics.append(line.strip())
        if "[NetworkTrace] " not in line:
            continue
        try:
            record = json.loads(line.split("[NetworkTrace] ", 1)[1])
            details = record["details"]
            event = record["event"]
            api = details.get("api", "")
        except (ValueError, KeyError, TypeError, AttributeError):
            malformed += 1
            continue
        stamp = {key: record.get(key) for key in ("seq", "ms", "pid", "thread")}
        if event == "api_invoked":
            last_call = {**stamp, "api": api, "registers": details.get("args_r0_r3")}
            if "[GKMatch sendData" in api:
                counts["send_api_invocations"] += 1
            if api == "[GameCenterSupport setMyMatch:]":
                registers = details.get("args_r0_r3", [])
                match_installed |= len(registers) >= 3 and registers[2] != 0
        if event == "api_returned" and api == "[GKLocalPlayer isAuthenticated]":
            authenticated |= details.get("return_r0_r1", [0])[0] != 0
        if event == "callback_invoked" and api == "[GameCenterSupport match:didReceiveData:fromPlayer:]":
            counts["game_receive_delegate_invocations"] += 1
        if event == "callback_invoked" and api == "[GameCenterSupport match:player:didChangeState:]":
            counts["game_player_state_delegate_invocations"] += 1
        if event == "gamekit_request":
            requests.append({**stamp, **details})
        if event == "gamekit_completion_error":
            errors.append({**stamp, **details})
        if event == "gamekit_transport":
            if details.get("kind") == "state":
                states.append({**stamp, **details})
            else:
                counts["transport_" + details.get("delivery", "unknown") + "_events"] += 1
        truncated |= event == "trace_truncated"
    return {
        "authenticated_observed": authenticated,
        "game_installed_non_nil_match_observed": match_installed,
        "counts": dict(counts), "requests": requests, "completion_errors": errors,
        "transport_states": states, "last_observed_api": last_call,
        "panic_markers": panics, "truncated": truncated, "malformed_records": malformed,
        "visible_same_lobby_and_mutual_players": "requires independent visual verification",
        "multiplayer_hunt": "not evaluated",
    }


def self_test():
    def record(event, details):
        return "[NetworkTrace] " + json.dumps(dict(event=event, details=details, seq=1, ms=1))
    result = analyze([
        record("api_returned", {"api": "[GKLocalPlayer isAuthenticated]", "return_r0_r1": [1, 0]}),
        record("api_invoked", {"api": "[GameCenterSupport setMyMatch:]", "args_r0_r3": [1, 2, 3, 4]}),
        record("callback_invoked", {"api": "[GameCenterSupport match:didReceiveData:fromPlayer:]"}),
        record("gamekit_completion_error", {"code": 3, "reason": "HostLost"}),
        record("trace_truncated", {}), "[NetworkTrace] {", "Panic at example",
    ])
    assert result["authenticated_observed"] and result["game_installed_non_nil_match_observed"]
    assert result["counts"]["game_receive_delegate_invocations"] == 1
    assert result["completion_errors"][0]["code"] == 3
    assert result["malformed_records"] == 1 and result["truncated"]
    assert result["panic_markers"] and "requires" in result["visible_same_lobby_and_mutual_players"]
    assert not analyze([])["game_installed_non_nil_match_observed"]
    print("Lobby log analyzer self-test passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", nargs="*", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    if args.logs:
        results = {}
        for path in args.logs:
            with path.open(errors="replace") as stream:
                results[str(path)] = analyze(stream)
        print(json.dumps(results, ensure_ascii=False, indent=2))
    elif not args.self_test:
        parser.error("supply log paths or --self-test")


if __name__ == "__main__":
    main()
