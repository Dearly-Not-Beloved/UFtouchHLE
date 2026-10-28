# IOS_SAVE_DIFF_REPORT.md - iOS MHP2G save differential report

> Status: 2026-10-28 — inner layer SOLVED (table perm + LCG, PSP-homologous), controlled 3-sample diff DONE, checksum F = remaining blocker.
> Method: byte-level, block-level and statistical diff of outer ciphertext, inner ciphertext and inner plaintext.

---

## 1. Samples available

| Sample | Origin | Outer ciphertext | Inner ciphertext | Inner plaintext | Notes |
|---|---|---|---|---|---|
| s1/s2/s3 (sample1-3.dat) | user-uploaded controlled saves | yes | yes (.inner.bin) | yes (.plain.bin) | s2/s3 same session pair |
| male.dat / female.dat | real-device exports | yes | yes | yes | /tmp/mhp2g_analysis/user_samples/ |
| run24 save | in-game save | savedata_write_stream.bin | cccrypt_plain.bin | plain_full.bin | archived in samples_run24/ |
| k73 PSP save | PPSSPP | n/a (PSP format) | n/a | /tmp/MHP2NDG.t2.plain | PSP plaintext reference |

## 2. Outer-layer diff (write stream vs re-encryption)

- openssl aes-128-cbc (key `6c7336285b66245d3d7e49286e663238`, iv `3b6c6b6e63283b665f776e5839383c69`) encrypt(cccrypt_plain.bin) == savedata_write_stream.bin — byte-identical. Outer layer fully deterministic.

## 3. Inner-layer crack (s2/s3 same-session differential)

- s2 vs s3 (same session): recovered per-block betas — **upper16 identical across the whole save, lower16 all different** → exactly the "two-LCG-stream" signature later confirmed by beta-recovery from zero-plaintext blocks.
- beta = lower + (upper<<16); `lower*0x215F %0xFF8F`, `upper*0xDFA3 %0xFFEF` (params identical to PSP; permutation table differs).
- Decrypt: `p = T2[T2[c] ^ beta_byte]`; Encrypt: `c = T1[T1[p] ^ beta_byte]`. T1@file 0x5f27d0, T2@0x5f28d0 (inverses, verified T2[T1[i]]==i).
- Closed loop: decrypt(encrypt(x)) == x, **0-byte diff on all 4 real saves**.

## 4. Game load-decrypt mechanism (WLOG + beta-recovery, verified)

- Game recovers beta from ciphertext block at offset **0xDFC00** (zero-plaintext anchor): `beta = T2[c_DFC00] ^ T1[0]`.
- Then runs the LCG **backward 0xDFC00/4 = 229120 steps** to get block-0 beta, then decrypts the whole save from block 0.
- Verified: `LCG^229120(block0 beta) == game-recovered beta` for both the random-beta test save (0xf94d64cb→0xcae1be89) and the male save (0x1ddbd6c0→0x90279d3b).
- Simulated full decryption of the random-beta save with recovered beta matches the real plaintext **100.00%** (all 1,483,008 bytes).
- Save tail u32 is NOT the seed: changing it (0x11223344) leaves the game's beta unchanged.

## 5. Load-verification experiments (all deployed to touchHLE, reproducible)

| Save | Change vs male | Game result |
|---|---|---|
| male original | none | **loads OK** (no alert) |
| roundtrip_male_randbeta | same plaintext, new random beta | corrupt ×N |
| seedtest_11223344 | inner tail u32 = 0x11223344 | corrupt; game beta unchanged |
| exp_tailzero | inner tail 30B zeroed | corrupt ×2 |
| exp_body1b | 1 byte at offset 0x2000 flipped | corrupt ×2 |
| exp_tail82 / exp_tail20 | tail 8B+2B only / tail 20B only | pending test |

=> Checksum covers **body + tail region**; it is a function of the **inner ciphertext** (plaintext-identical re-encryption still fails). Not standard SHA1/MD5/SHA256 (no hash constants in binary, no CC_* calls logged).

## 6. Plaintext structural features (current, supersedes older notes)

- Offset 0x0000: `00 01 04 04` (version marker); 0x1000: UTF-16LE character name ("殲滅のレウス…"/"unknown…" on real saves).
- 0x5351+: 15-byte-spaced ff runs (bitmap-like); ~99.9% zero.
- **Tail 30B = tail8 | tail20 | tail2**: tail2's last byte is always `0x77` (all 4 saves); tail8/tail20 vary per save; s2/s3 share `ff d0` at tail8 bytes 4-5.

## 7. PSP structural comparison (for reference)

- PSP 1,483,004B, head `00 03 07 07`; tail 12B metadata + 20B SHA1(data[:-12] + salt `S)R?Bf8xW3#5h9lGU8wR`) — verified exactly on k73.
- ~91% of bytes identical between k73 (PSP) and male (iOS) plaintexts; platform-specific header + tail differ.

## 8. Checksum F — current blocker

- F(inner ciphertext body) must equal stored tail20; algorithm unknown. No standard-hash constants in binary. Two 16-byte ASCII strings `f09qfkn#olsp9fkn` / `b#hl-*pa-lspzm ` sit right after the AES key/iv strings (candidate salts/second key pair) — SHA1(body+salt) did not match any tested salt/range/endianness variant.
- Recommended attacks: (a) CPU-level PC capture to isolate the checksum routine (runs after inner decrypt, before alert); (b) static xrefs of those two strings; (c) deploy roundtrip_male.dat (byte-identical to original) as a baseline sanity check.

## 9. Verified facts only

- Outer AES-128-CBC fixed key/iv: VERIFIED (byte-identical reproduction)
- Inner = table perm + dual LCG (params PSP-identical, table differs): VERIFIED (closed-loop 0-diff on 4 saves)
- Game load-decrypt: beta from 0xDFC00, LCG back 229120 steps, decrypt whole save: VERIFIED (100% plaintext match)
- Original male save loads in touchHLE; any re-encryption rejected: VERIFIED (in-game)
- Checksum covers body+tail, function of inner ciphertext: VERIFIED (tailzero/body1b experiments)
- Checksum algorithm F identified: NOT YET (blocker)
- PSP SHA1 tail verified; iOS tail not a simple SHA1 of the file: VERIFIED
