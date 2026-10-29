#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Looks up games' covers on IGDB, for the Android launcher's posters.

IGDB's API needs Twitch developer credentials, which mustn't be shipped in
the app, but its cover images are public. So this is run by hand, and the
cover image IDs it finds go into the IGDB table in Posters.java; the
launcher then downloads the images directly.

Usage: dev-scripts/igdb-covers.py [OUTPUT.json]

The Twitch Client ID and Secret are asked for (the secret without echoing)
and only used for this run. For each game in GAMES below, the output lists
IGDB's best matches with their platforms and cover IDs, so the right one can
be picked by hand.
"""

import getpass
import json
import sys
import urllib.parse
import urllib.request

# Bundle identifier, then the names to search IGDB for.
GAMES = [
    ("jp.co.capcom.mhp2gjp", ["Monster Hunter Portable 2nd G"]),
    ("jp.co.capcom.mhp2gwd", ["Monster Hunter Freedom Unite"]),
    ("com.rockstargames.gta3vc", ["Grand Theft Auto: Vice City"]),
    ("jp.co.capcom.deadrisingus", ["Dead Rising Mobile", "Dead Rising"]),
    ("com.ea.nfss2.inc", ["Shift 2: Unleashed"]),
    ("com.com2us.feanor2", ["Chronicles of Inotia: Legend of Feanor", "Inotia"]),
    ("com.com2us.inotia", ["Inotia 2: A Wanderer of Luone", "Inotia 2"]),
    ("jp.co.capcom.biovscn", ["Resident Evil: The Mercenaries Vs.", "Resident Evil Mercenaries Vs"]),
]


def post(url, headers=None, body=None):
    request = urllib.request.Request(
        url, data=body.encode() if body is not None else b"", headers=headers or {}
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.load(response)


def main():
    output = sys.argv[1] if len(sys.argv) > 1 else "igdb-covers.json"
    client_id = input("Twitch Client ID: ").strip()
    secret = getpass.getpass("Twitch Client Secret (input is hidden): ").strip()
    token = post(
        "https://id.twitch.tv/oauth2/token?"
        + urllib.parse.urlencode(
            {
                "client_id": client_id,
                "client_secret": secret,
                "grant_type": "client_credentials",
            }
        )
    )["access_token"]
    headers = {"Client-ID": client_id, "Authorization": "Bearer " + token}

    results = {}
    for bundle_id, names in GAMES:
        matches = []
        for name in names:
            query = name.replace('"', "")
            games = post(
                "https://api.igdb.com/v4/games",
                headers,
                f'search "{query}"; '
                "fields name, cover.image_id, platforms.abbreviation, "
                "first_release_date, url; limit 6;",
            )
            for game in games:
                matches.append(
                    {
                        "search": name,
                        "name": game.get("name"),
                        "platforms": [
                            p.get("abbreviation") for p in game.get("platforms", [])
                        ],
                        "cover": game.get("cover", {}).get("image_id"),
                        "url": game.get("url"),
                    }
                )
        results[bundle_id] = matches
        print(f"{bundle_id}: {len(matches)} matches")

    with open(output, "w") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print("Wrote", output)


if __name__ == "__main__":
    main()
