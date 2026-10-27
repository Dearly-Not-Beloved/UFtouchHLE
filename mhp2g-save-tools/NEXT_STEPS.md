# NEXT_STEPS.md - MHP2G PSP <-> iOS save converter status

> Date: 2026-10-28 (after in-game load failure investigation). Companion to IOS_SAVE_DYNAMIC_ANALYSIS.md / IOS_SAVE_DIFF_REPORT.md / PSP_IOS_FORMAT_COMPARISON.md

---

## STATUS: crypto fully cracked; game load-verify mechanism fully understood; the ONLY remaining blocker is the save checksum algorithm F

### The complete iOS save crypto chain (all verified)

1. **Outer layer**: AES-128-CBC + PKCS7.
   - key  = `6c7336285b66245d3d7e49286e663238` (ASCII `ls6([f$]=~I(nf28`)
   - iv   = `3b6c6b6e63283b665f776e5839383c69` (ASCII `;lknc(;f_wnX98<i`)
   - Verified: openssl-encrypted plaintext == savedata_write_stream.bin byte-identical.

2. **Inner layer**: byte-wise table permutation + beta keystream (PSP-homologous):
   - Encrypt per byte: `c = T1[T1[p] ^ beta_byte]`   (T1 from iOS binary file 0x5f27d0)
   - Decrypt per byte: `p = T2[T2[c] ^ beta_byte]`   (T2 = inverse, file 0x5f28d0)
   - beta (32-bit per 4-byte block) = lower + (upper << 16); LCG: `lower=(lower*0x215F)%0xFF8F`, `upper=(upper*0xDFA3)%0xFFEF` (identical params to PSP; table differs: iOS T1[0]=0x3a vs PSP TABLE[0]=0xa6).
   - Closed-loop proof: encrypt(decrypt(x))==x with 0-byte diff. Four real saves all decrypt to valid plaintext.

### NEW: game in-memory load/decrypt mechanism FULLY understood (verified via WLOG + beta-recovery)

- Game decrypts the save **starting at plaintext buffer 0x345dae0**; the decrypted window covers at least offsets 0..0xE6044 (WLOG range), and our simulated decryption of the **entire** save (all 1,483,008 B) with the recovered beta matches the real plaintext **100.00%**.
- **Beta[0] the game uses is recovered from ciphertext block at file offset 0xDFC00** (a zero-plaintext region): `beta_DFC00 = T2[c_DFC00] ^ T1[0]`.
- The game then **runs the LCG backward** 0xDFC00/4 = 229120 steps to obtain block-0 beta, and decrypts the whole save from block 0. Verified:
  - `LCG^229120(my block0 beta 0xf94d64cb) = 0xcae1be89` == game-recovered beta (rnd-beta test)
  - `LCG^229120(male block0 beta 0x1ddbd6c0) = 0x90279d3b` == game-recovered beta (male save)
- The save tail u32 is NOT the seed: changing it (seedtest) leaves the game's beta unchanged.
- **Critical fact**: the ORIGINAL male save (real-device export) LOADS in touchHLE without any "损坏" alert (no UIAlertView in log), while every re-encrypted save (same plaintext, different beta) is rejected as corrupt. Conclusion: **the game verifies a checksum that is a function of the inner ciphertext (or part of it), so any beta change invalidates it.**

### Checksum-region experiments (all deployed, all reproducible)

| Save | Modification | Game result |
|---|---|---|
| male original | none | loads OK (no alert) |
| roundtrip (same plaintext, random beta) | re-encrypted only | corrupt ×N |
| seedtest (tail u32 = 0x11223344) | tail only | corrupt (game beta unchanged) |
| exp_tailzero (tail 30B zeroed) | checksum region zeroed | corrupt ×2 |
| exp_body1b (1 byte at 0x2000 flipped) | body only | corrupt ×2 |
| exp_tail82 / exp_tail20 | (pending test) | - |

=> The checksum covers the **body** (1-byte flip fails) AND the **tail region** (tail-zero fails). It is NOT a standard SHA1/MD5/SHA256: no initial constants (0x67452301...) nor SHA1 round constants (0x5A827999...) found anywhere in the binary, and no CC_SHA* calls appear in touchHLE logs.

### Tail region structure (inner plaintext, last 30 bytes)

- `tail30 = tail8 | tail20 | tail2`
- tail2: last byte is ALWAYS `0x77` across all 4 saves; byte before varies.
- tail8 and tail20 differ per save; s2/s3 (same session) share `ff d0` at bytes 4-5 of tail8.
- Adjacent to the AES key/iv strings in the binary are two 16-byte ASCII strings `f09qfkn#olsp9fkn` and `b#hl-*pa-lspzm ` (likely salts / second key pair) — SHA1(body + salt) did NOT match for any tested salt/range/endianness variant.

## Converter v0 (mhp2g_save.py) - DONE, verified (crypto round-trip)

- `inspect <path>` - platform auto-detect + version header + character name.
- `convert --from psp --to ios --input <PSP BIN> --output <out.dat>` and reverse.
- Artifact: /Users/wesley/Documents/touchHLE/mhp2g-save-tools/mhp2g_save.py

## THE blocker & next steps (ranked)

1. **BREAK checksum algorithm F**: F(inner ciphertext body) == tail20 stored. With F unknown, no modified save can load. Recommended attack paths:
   a. Dynamic: hook reads of the ciphertext staging area (0x31561000) and the main buffer during load; with PC/instruction capture in the dynarmic wrapper, isolate the checksum routine (it runs after inner decryption, before the alert). We already dump at UIAlertView; extend to dump guest heap at alert time and diff against a good load.
   b. Static: HaXe bytecode is interpreted; look for the SHA1/checksum implementation in the HaXe-embedded data (tagged-int constant pool), or the string `f09qfkn#olsp9fkn`/`b#hl-*pa-lspzm ` xrefs (who references them).
   c. If F turns out to be a plaintext checksum instead of ciphertext checksum: re-verify by writing a save whose plaintext is byte-identical to male but encrypted with the ORIGINAL male beta (roundtrip_male.dat exists, not yet deployed).
2. **Then**: converter must recompute tail30 for any re-encrypted save. PSP saves already carry a SHA1 tail; once F is known, iOS writer becomes complete.
3. **iOS -> PSP direction** + real-device PPSSPP verification (user-side, PPSSPP install not allowed here).
4. Rust implementation + CLI + unit tests (task book phases 2-6).
5. touchHLE integration (phase 8): import/export UI, auto-backup, restore-on-failure.

## Diagnostic instrumentation added this round (in tree, uncommitted until reviewed)

- `src/frameworks/uikit/ui_view/ui_alert_view.rs`: on any UIAlertView (e.g. "游戏存档已损坏") dump the captured CPU read/write logs + 3 guest-memory snapshots (0x345dae0 plaintext window, 0x31561000 ciphertext staging, 0x313f6000 AES input) via `get_bytes_fallible` (fixed the private `bytes()` compile error; binary now contains the instrumentation).

## Explicit non-goals (per user constraints)

- No blind brute-force of LCG/XOR/permutation combos (cracked; closed-loop verified).
- No fake data masquerading as game-verified results (all claims are byte-level verified or explicitly marked as pending in-game test).
- No overwriting original saves (backups in /tmp/mhp2g_analysis/current_backup_before_exp.dat etc.).
- Real-device verification where required is clearly separated from static/emulated verification.
