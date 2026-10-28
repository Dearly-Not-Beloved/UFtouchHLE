# PSP_IOS_FORMAT_COMPARISON.md - PSP vs iOS MHP2G save format comparison

> **Correction (2026-10-28, later):** checksum F is solved and several
> conclusions below are wrong. The iOS save uses exactly the PSP scheme:
> SHA-1(plain[0:0x16A0DC] + "3Nc94Hq1zOLh8d62Sb69") at 0x16A0E8, and the
> XOR-stream seed is the last 4 bytes (stored through the table twice), not a
> per-save beta "recovered from 0xDFC00". The game does call CC_SHA1. Re-encrypted
> saves failed only because the old converter didn't store the seed.
> Also, PSP MHP2G JP uses the *same* byte tables as iOS, not the mhef table
> (`A6 21 22 ...`) assumed below; only the salt differs. See
> README.md and NEXT_STEPS.md; this file is kept as a research log.

> Status: 2026-10-28 — both platforms' crypto FULLY solved; iOS inner = PSP-homologous table perm + LCG (table differs). Remaining blocker: iOS save checksum F (see NEXT_STEPS.md). Field mapping table is incomplete by design - only facts with evidence are listed.

---

## 1. Overview

| Aspect | PSP (MHP2G JP) | iOS (MHP2G JP, touchHLE) |
|---|---|---|
| Sample | k73.com save | touchHLE sandbox savedata.dat |
| File | ULUS10391/PARAM.SFO + save data | single savedata.dat |
| File size | 1,483,004B (decrypted) | 1,483,024B (outer ciphertext) / 1,483,008B (inner) |
| Outer crypto | KIRK AES-128-CBC (system layer) | CCCrypt AES-128-CBC (key/iv fixed, see IOS_SAVE_DYNAMIC_ANALYSIS.md) |
| Inner crypto | Capcom table perm + LCG | **same scheme; params identical, table differs** (iOS T1[0]=0x3a vs PSP TABLE[0]=0xa6) |
| Checksum | tail 20B = SHA1(data[:-12] + salt "S)R?Bf8xW3#5h9lGU8wR") | tail 30B = 8B | 20B | 2B; checksum algorithm F UNKNOWN (blocker) |
| Version header | 00 03 07 07 | 00 01 04 04 |
| Zero ratio | 98.44% | 99.94% |

---

## 2. Crypto layers (verified facts)

### PSP (fully solved)
- System layer: KIRK AES-128-CBC; game key JP = cd1f2059ae70ef68dca24513b45adb0a
- Game layer: table permutation + LCG
  - encrypt: c[i] = T1[ T1[p[i]] XOR beta[i] ]
  - decrypt: p[i] = T2[ T2[c[i]] XOR beta[i] ]
  - beta[i] = lower[i] + (upper[i] << 16)
  - lower = (lower * 0x215F) % 0xFF8F
  - upper = (upper * 0xDFA3) % 0xFFEF
  - seed = last 4B, permuted twice through T2 table
- SHA1 over data[:-12] + salt, stored in last 20B
- Verified: k73 save SHA1 exactly matches; 370751/370751 blocks decrypt correctly; zero ratio 98.44%

### iOS (fully solved 2026-10-28)
- Outer: CCCrypt AES-128-CBC, key = ls6([f$]=~I(nf28 (hex 6c7336285b66245d3d7e49286e663238), iv = ;lknc(;f_wnX98<i (hex 3b6c6b6e63283b665f776e5839383c69)
- Outer verified: encrypt(cccrypt_plain.bin) == savedata_write_stream.bin
- Inner ciphertext = 1,483,008B; inner plaintext 99.94% zero
- Inner = same Capcom scheme as PSP: `p=T2[T2[c]^beta]` / `c=T1[T1[p]^beta]`, beta=lower+(upper<<16), LCG lower*0x215F %0xFF8F, upper*0xDFA3 %0xFFEF; **T1/T2 are the iOS tables (file 0x5f27d0/0x5f28d0), NOT the PSP table**
- Closed loop verified: decrypt(encrypt(x))==x, 0-byte diff, on all 4 real saves
- beta[0] is per-save random (NOT derived from tail u32; tail u32 is NOT the seed — changing it leaves game beta unchanged)
- **Game load-decrypt**: recovers beta from ciphertext @0xDFC00, runs LCG backward 229120 steps to block-0 beta, decrypts whole save; simulated decrypt matches real plaintext 100.00%
- **Original male save loads in touchHLE; any re-encrypted save is rejected** => checksum F over inner ciphertext body; F UNKNOWN (no standard-hash constants in binary)
- Two 16B ASCII strings `f09qfkn#olsp9fkn` / `b#hl-*pa-lspzm ` sit after the key/iv strings (candidate salts / second key pair; SHA1 tests negative)

---

## 3. Structural comparison (confirmed fields only)

| Field | PSP offset (k73) | iOS offset | Status |
|---|---|---|---|
| version header | 0x0000: 00 03 07 07 | 0x0000: 00 01 04 04 | CONFIRMED different |
| character name | present (UTF-16, found in plaintext) | 0x1000: UTF-16 "111" | CONFIRMED location on iOS, PSP offset TBD |
| quest name ref | m60038.mib @ 0x148b00 | not found in same layout | PSP-specific |
| tail seed (4B) | last 4B (permuted via T2) | **NOT a seed** — changing it leaves the game's beta unchanged (verified) | CONFIRMED not seed on iOS |
| checksum tail | SHA1(data[:-12]+salt), 20B | iOS tail 30B = 8B | 20B | 2B (last byte always 0x77); algorithm F UNKNOWN | iOS checksum = blocker |

---

## 4. Known differences (do NOT copy fields blindly)

1. **Size and layout**: PSP 1,483,004B vs iOS inner 1,483,008B - different offsets for the same logical fields are expected; byte-copy is invalid.
2. **Version header** differs (00 03 07 07 vs 00 01 04 04): the game likely switches serialization layout by version.
3. **Zero regions** differ (98.44% vs 99.94%): iOS plaintext is sparser; the iOS plaintext is built from a template (memcpy 2 from HaXe heap 426,044B onto offset 0x1000).
4. **Seed handling**: PSP seed = tail 4B permuted via table; **iOS tail u32 is NOT a seed** (changing it leaves game beta unchanged; beta[0] is per-save random, recovered by the game from ciphertext @0xDFC00).
5. **Checksum**: PSP SHA1+salt verified; iOS tail 20B checksum F over the inner ciphertext — UNKNOWN algorithm (no standard-hash constants in binary; any re-encryption is rejected in-game). Must be cracked before converter output is loadable.

---

## 5. Field mapping table (to be completed)

To be completed only with evidence from controlled 3-sample diff (see IOS_SAVE_DIFF_REPORT.md):
- character: name, gender, appearance
- playtime, zenny, resources
- inventory (bag), box (warehouse)
- equipment + upgrade states
- skills
- completed quests, unlock states
- hunter rank progress
- guild card
- game settings

Current blocker: **iOS save checksum F (over inner ciphertext) is not cracked** — until F is known, no re-encrypted save loads in-game, so no field offset can yet be verified in-game. Once F is cracked, controlled single-value-change saves become possible and the mapping can be completed.

---

## 6. Risks

- Mapping by assumption (same layout as PSP) is explicitly rejected per task rules; all differences above prove layouts differ.
- Unknown-field loss: converting unknown iOS fields to PSP (or vice versa) must produce a migration report, not silent truncation.
- Version mismatch: PSP saves from non-JP regions (ULUS10391 uses a different KIRK key, already archived at /tmp/ulus10391.key) may use different game-layer tables - verify region before converting.
