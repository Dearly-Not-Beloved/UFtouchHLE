# IOS_SAVE_DIFF_REPORT.md - iOS MHP2G save differential report

> Status: partial. Outer layer fully solved. Controlled 3-sample experiment (original / re-save / single-value-change) NOT yet complete - only naturally-occurring saves captured so far.
> Method: byte-level, block-level and statistical diff of outer ciphertext, inner ciphertext and inner plaintext.

---

## 1. Samples available

| Sample | Origin | Outer ciphertext | Inner ciphertext | Inner plaintext | Notes |
|---|---|---|---|---|---|
| run13 save i1 | in-game save | yes | yes (from outer decrypt) | no | upper16 stable, lower16 varies |
| run13 save i2 | in-game save | yes | yes | no | seed 0x33df |
| run24 save | in-game save | savedata_write_stream.bin | cccrypt_plain.bin | plain_full.bin | seed 0x33ac |
| k73 PSP save | PPSSPP | n/a (PSP format) | n/a | /tmp/MHP2NDG.t2.plain | PSP plaintext reference |

Note: /tmp/touchhle_dump was cleared on each restart; run13 intermediates were lost. Current archive: /tmp/mhp2g_analysis/samples_run24/ (plain_full.bin, cccrypt_plain.bin, savedata_write_stream.bin, memcpy_dst.bin).

---

## 2. Outer-layer diff (savedata_write_stream vs re-encryption)

- openssl aes-128-cbc -K 6c7336285b66245d3d7e49286e663238 -iv 3b6c6b6e63283b665f776e5839383c69 encrypt(cccrypt_plain.bin) == savedata_write_stream.bin
- Result: byte-identical (True). Outer layer fully deterministic given inner ciphertext.

---

## 3. Inner-ciphertext block diff (run13 i1 vs i2, 4-byte blocks)

Historical result (run13): comparing two different saves' inner ciphertexts:
- 99.998% of 4-byte blocks have identical upper 16 bits
- lower 16 bits change nearly everywhere (seed-driven)

Interpretation: the keystream upper half is a fixed sequence independent of seed; the lower half is generated from the per-save seed. The 4-byte block structure (upper = fixed, lower = seeded) is the dominant pattern of the inner layer.

## 4. Inner-ciphertext vs plaintext relationship (run24, single save)

- plain_full.bin XOR cccrypt_plain.bin[:1483008] = keystream
- Keystream stats: 370752 4-byte blocks; lower 16 unique values ~9612; upper 16 unique values 65519
- Lower 16: strict local periodicity. Value 0x4b0c appears at blocks [5113, 14459] (spacing 9346) in run24 and at [1, 9347, 18693, ...] in run17 - period component confirmed, phase shifts with seed.
- Upper 16: no repeat >2 times in first 20000 terms; identical across saves.

## 5. Plaintext structural features (run24 plain_full.bin)

- Offset 0x0000: 00 01 04 04 (version marker)
- Offset 0x1000: UTF-16LE "111" (character name, sample save)
- 0x5351+: 15-byte-spaced ff runs (bitmap-like)
- 0x16a0e0: 6B 1b 0f 1b 2a ea 0a
- 0x16a0e8: 22B a9 49 7d 8f b2 50 d2 2c 64 f9 37 59 db 2f fe 2f f8 e7 12 b6 91 08 (last 20B suspected SHA1-like checksum)
- Tail u32 0x0891 (0x16a0fc)
- 99.94% zero -> most of the 1.48MB is reserved/empty

## 6. PSP plaintext structural features (k73, for comparison)

- /tmp/MHP2NDG.t2.plain = 1,483,004B, zero ratio 98.44%
- head 4B: 00 03 07 07
- character name "111" found at plaintext 0x148b00 area (quest name m60038.mib at 0x148b00; char data elsewhere)
- tail 20B = SHA1(data[:-12] + salt "S)R?Bf8xW3#5h9lGU8wR") - verified exactly on k73 save
- seed = last 4B, permuted twice through T2 table

## 7. What remains for a true controlled 3-sample diff

The task book requires three controlled iOS saves:
1. original save (untouched)
2. re-save with no game-state change
3. save after changing exactly one game value

Not yet achieved: current captures are natural saves after the game auto-loads; no controlled single-value mutation was performed yet. This needs:
- a deliberate single-value change in-game (e.g. carry item count / zenny), then save
- diff of plaintexts 1 vs 2 (expect: timestamp/seed region only), 2 vs 3 (expect: exactly the changed field(s) + seed region)

Block-level and statistical diff tooling exists; run it on controlled samples once produced.

## 8. Cross-save keystream differential (next main attack)

With 3+ seeds (S1, S2, S3) and their lower16 sequences L1, L2, L3:
- If lower16 = PRNG(seed, i), test linearity: L1[i] XOR L2[i] should be a function of S1 XOR S2 if the PRNG is linear (LCG/xorshift-like)
- If PRNG is non-linear (e.g. table-driven), collect (seed, L[0..8]) pairs and brute-force candidate structures offline
- Upper16: verify cross-save identity on all blocks, not just first 20000

## 9. Verified facts only (do not conflate)

- Outer AES-128-CBC with fixed key/iv: VERIFIED (byte-identical reproduction)
- Inner ciphertext = outer AES plaintext: VERIFIED
- Keystream = plaintext XOR inner ciphertext: VERIFIED by construction
- Lower16 9346-period component: VERIFIED (two saves, exact spacing)
- Lower16 global 9346 period: DISPROVEN
- PSP inner scheme (table perm + LCG) applies to iOS inner: DISPROVEN
- Inner PRNG identified: NOT YET
