# NEXT_STEPS.md - MHP2G PSP <-> iOS save converter status

> Date: 2026-10-28 (after inner-layer crack). Companion to IOS_SAVE_DYNAMIC_ANALYSIS.md / IOS_SAVE_DIFF_REPORT.md / PSP_IOS_FORMAT_COMPARISON.md

---

## STATUS: iOS inner-layer encryption FULLY CRACKED (verified end-to-end)

### The complete iOS save crypto chain (all verified)

1. **Outer layer**: AES-128-CBC + PKCS7.
   - key  = `6c7336285b66245d3d7e49286e663238` (ASCII `ls6([f$]=~I(nf28`)
   - iv   = `3b6c6b6e63283b665f776e5839383c69` (ASCII `;lknc(;f_wnX98<i`)  <- note: 16 bytes; older notes had a stray `f`
   - Verified: openssl-encrypted plaintext == savedata_write_stream.bin byte-identical.

2. **Inner layer**: byte-wise table permutation + beta keystream, **exactly PSP-homologous**:
   - Encrypt per byte: `c = T1[T1[p] ^ beta_byte]`   (T1 from iOS binary file 0x5f27d0)
   - Decrypt per byte: `p = T2[T2[c] ^ beta_byte]`   (T2 = inverse, file 0x5f28d0; T2[T1[i]]==i verified)
   - beta (32-bit per 4-byte block) = lower + (upper << 16)
   - lower LCG: `lower = (lower * 0x215F) % 0xFF8F`
   - upper LCG: `upper = (upper * 0xDFA3) % 0xFFEF`
   - LCG params are IDENTICAL to PSP; the permutation table is NOT the PSP one (iOS T1[0]=0x3a, PSP TABLE[0]=0xa6).
   - beta[0] is a per-save random value (NOT derived from the tail u32 seed; verified over 4 samples).
   - beta[0] recovery from any clean (zero-plaintext) block is trivial via T2.

3. **Closed-loop proof**: s2 inner ciphertext -> inner decrypt -> inner encrypt == identical (0 byte mismatch). Four real saves (s2/s3/male/female) all decrypt to valid plaintext: header `00 01 04 04`, `ff ff ff` runs @0x5351, UTF-16 character name @0x1000.

4. **PSP game-layer** (already cracked, re-verified today):
   - `p = TABLE[TABLE[c]] ^ beta` (XOR outside permutation), TABLE embedded in /tmp/psp_decrypt.py (TABLE[0]=0xa6), seed = last 4B permuted 2x.
   - PSP system layer: KIRK AES-128-CBC via /tmp/mhef (verified on k73 save).
   - Our converter's PSP decrypt now matches psp_decrypt.py reference byte-identical.

### PSP vs iOS plaintext layout

- Lengths: PSP 1,483,004B vs iOS 1,483,008B (4B tail difference).
- ~91% of bytes identical between k73 (PSP) and male (iOS) saves; header fields differ (PSP `00 03 07 07` vs iOS `00 01 04 04`); character-name UTF-16 at 0x1000 on both.
- Tail region: PSP has 12B metadata + SHA1-like 20B; iOS has 8B + 22B checksum-like region (not a simple SHA1 of the file; games may not verify it - needs in-game test).

## Converter v0 (mhp2g_save.py) - DONE, verified

- `inspect <path>` - platform auto-detect (iOS len==1483024), prints version header + character name.
- `convert --from psp --to ios --input <PSP BIN> --output <out.dat>` - system+game decrypt, plaintext-copy into iOS template (keeps iOS header 0x0-0x1000 and tail 0x16a0d4+), inner encrypt, AES outer encrypt. Round-trip verified.
- `convert --from ios --to psp ...` - reverse (iOS decrypt -> plaintext-copy into PSP template -> PSP game-layer encrypt). Round-trip needs in-game verification.
- Artifact: /Users/wesley/Documents/touchHLE/mhp2g-save-tools/mhp2g_save.py
- Sample output: converted_k73_to_ios.dat (k73 "jun zai" save converted to iOS).

## Next steps (ranked)

1. **REAL GAME TEST (most important)**: put converted_k73_to_ios.dat into touchHLE sandbox Documents/ (back up current savedata.dat first), restart MHP2G, verify "jun zai" character loads and can be played/saved. This proves the byte-copy conversion works or reveals which fields need mapping.
2. **Field mapping**: with the converter round-tripping, build a field-level map (name/gender/zenny/items/equipment/quests) using controlled single-value-change saves (user triggers save after changing one value).
3. **iOS -> PSP direction**: verify the converted PSP save loads in PPSSPP on a real device (user-side test; PPSSPP install not allowed here). Static verification + round-trip done here.
4. **Rust implementation + CLI** (per task book): mhp2g-save-tools/src/ with unit tests (io_save round trip, psp_game round trip).
5. **touchHLE integration** (later): import/export UI, auto-backup, restore-on-failure.

## Explicit non-goals (per user constraints)

- No blind brute-force of LCG/XOR/permutation combos (no longer needed - cracked).
- No fake data masquerading as game-verified results (all claims above are verified byte-level).
- No overwriting original saves (backups in /tmp/mhp2g_analysis/, /tmp/mhp2g_backup_20261027_205842/).
