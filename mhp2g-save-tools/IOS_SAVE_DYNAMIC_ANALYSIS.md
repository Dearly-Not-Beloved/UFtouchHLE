# IOS_SAVE_DYNAMIC_ANALYSIS.md - iOS MHP2G save dynamic analysis

> Status: ongoing. Outer encryption fully solved. Inner algorithm NOT determined.
> Dates: 2026-10-27 ~ 2026-10-28
> Method: touchHLE source instrumentation + dynamic capture + static disassembly

---

## 1. Conclusion summary (by evidence strength)

| Conclusion | Status | Evidence |
|---|---|---|
| Outer encryption = AES-128-CBC + PKCS7 | SOLVED | CCCrypt args captured; openssl byte-identical reproduction |
| Outer key / iv are fixed constants | CONFIRMED | binary VA 0x51e297 / 0x51e276 |
| Save size = 1,483,024B (incl. 16B PKCS7 padding) | CONFIRMED | multiple captures |
| Inner ciphertext = outer AES plaintext (1,483,008B) | CONFIRMED | CCCrypt op=1 dataIn dump |
| Inner plaintext = sparse structure (99.92% zero) | CONFIRMED | plain_full.bin entropy 0.01 |
| Keystream upper 16 bits stable across saves | CONFIRMED | run17/run22/run24 identical |
| Keystream lower 16 bits vary across saves | CONFIRMED | seed-driven |
| Keystream lower 16 bits have 9346-period component | CONFIRMED | 0x4b0c spaced exactly 9346, multiple saves |
| Inner encryption write path: NOT CPU/memcpy/memmove/mem.write | CONFIRMED | run18-24 all hooks excluded |
| Inner encryption algorithm (PRNG/table) | NOT DETERMINED | all LCG brute-force failed |

---

## 2. Save flow timeline (run24 capture)

```
[LOAD]   CCCrypt op=0 dataIn=0x345dae0(1483008B) -> dataOut=0x313f6000
         -> 0x313f6000 = outer-decrypted output = inner ciphertext (from prev save)
[LOAD]   game inner-decrypt -> plaintext -> game state
[MODIFY] game serializes plaintext into main buffer 0x345dae0 (UTF-16 char name @0x1000)
[MEMCPY] 1: 0x31561000 -> 0x345dae0, 1483008B (outer ciphertext copy-back, after CCCrypt)
[MEMCPY] 2: 0x5c2a9bc -> 0x345eae0, 426044B (HaXe heap template overwrites plaintext segment)
[CLEAR]  CPU r/w logs cleared (at memcpy 2)
[ENCRYPT] inner: plaintext(0x345dae0 region) -> inner ciphertext(0x313f6000)  <- WRITE PATH UNKNOWN
[OUTER]  CCCrypt op=1 dataIn=0x313f6000(1483024B) -> dataOut=0x31561000
[WRITE]  posix write 1,483,024B -> savedata.dat
```

**Key fact**: at CCCrypt op=1, dataIn=0x313f6000 already contains the full inner ciphertext. The game generates it before op=1, but NONE of the hookable write paths (CPU callbacks, memcpy, memmove, mem::write) captured writes to the 0x313f6000 region. Inference: game native (ObjC/C) code writes via a direct host pointer; touchHLE instrumentation cannot intercept it.

---

## 3. Outer encryption (SOLVED)

- Algorithm: AES-128-CBC, options=1 (kCCOptionPKCS7Padding)
- key (16B): 6c 73 36 28 5b 66 24 5d 3d 7e 49 28 6e 66 32 38 = ls6([f$]=~I(nf28
- iv (16B): 3b 6c 6b 6e 63 28 3b 66 5f 77 6e 58 39 38 3c 69 = ;lknc(;f_wnX98<if
- key at VA 0x51e297 (first 16B); iv at VA 0x51e276 (first 16B)
- Verified: openssl aes-128-cbc encrypt(cccrypt_plain.bin) == savedata_write_stream.bin byte-for-byte (True)

---

## 4. Inner ciphertext and plaintext

| File | Size | Description |
|---|---|---|
| plain_full.bin | 1,483,008B | plaintext before inner encryption (dumped at memcpy-2 hook) |
| cccrypt_plain.bin | 1,483,024B | CCCrypt op=1 input (= inner ciphertext + 16B padding); first 1,483,008B = inner ciphertext |
| savedata_write_stream.bin | 1,483,024B | final write stream (outer ciphertext) |

**Plaintext structure (plain_full.bin)**:
- head 4B: 00 01 04 04 (version marker; PSP uses 00 03 07 07)
- @0x1000: UTF-16LE character name "111" (sample save)
- 99.94% zero bytes
- @0x5351 onward: many `ff` runs spaced 15 bytes apart (suspected bitmap)
- @0x16a0e0: 6B 1b 0f 1b 2a ea 0a (unknown field)
- @0x16a0e8: 22B a9 49 7d 8f b2 50 d2 2c 64 f9 37 59 db 2f fe 2f f8 e7 12 b6 91 08 (last 20B suspected SHA1-like checksum)
- tail: ... 91 08 00 00 (plaintext tail u32 = 0x0891)

**Inner ciphertext last 4B**: upper 16 bits always 0x7777, lower 16 bits change per save (this save: 0x33ac) - suspected seed storage (0x7777 = fixed salt prefix).

---

## 5. Keystream analysis (plaintext XOR inner ciphertext)

Keystream[i] = plain_full[i] XOR cccrypt_plain[i] (plaintext is 99.92% zero, so ciphertext equals the keystream at most positions).

- upper 16 bits: stable across saves, 65519 unique values, no short period (no value repeats >2 times in first 20000 terms)
- lower 16 bits: varies per save, ~9612 unique values, has a strict periodic component

**9346-period evidence (reproduced across saves)**:
- run17: 0x4b0c at blocks [1, 9347, 18693, 28039, 37385, ...] (spacing exactly 9346)
- run24: 0x4b0c at blocks [5113, 14459] (spacing 9346; starting phase shifts with seed)
- 9346 = 2 x 4673; 9346 divides (0xFF8F - 1) = 65422

**Excluded algorithms (do not retry)**:
1. PSP table permutation (T2 table @0x5F68D0) on inner: beta 4-byte mismatch, plaintext still entropy 7.996
2. PSP LCG params (0x215F/0xFF8F, 0xDFA3/0xFFEF): 0 hits in first 20 terms
3. 16-bit multiplicative LCG brute force (mod 0xFF8F, 0xFFEF, 0xFFFF, 0x10000, 0xFFFD, 0xFFEF-1, 0xFF8F-1; c=0 and c!=0 both tested): 0 hits
4. Full-sequence period 9346 check: False (0x4b0c period is local, not global)

---

## 6. Write-path exclusion experiments (run18-24)

| run | instrumentation | result |
|---|---|---|
| run18 | disabled dynarmic page_table (fastmem); CPU write hook logs 0x345dae0-0x3560a20 | 0 hits -> inner encryption is NOT CPU direct write |
| run19 | read hook on 0x345dae0 region | log flooded by unrelated runtime accesses |
| run20 | clear logs at memcpy-426KB, keep only encryption window | 2M write entries all plaintext serialization (0x345dae0 region); no 0x313f6000 writes |
| run21 | all large memcpy logs (any dst) | 19569 entries, none dst in 0x313fxxxx |
| run22 | independent r/w logs for 0x313f6000 region | writes only 178 entries (all at 0x313f5fxx tail-arg region, not ciphertext) |
| run23 | memmove hook | no large memmove into 0x313f region |
| run24 | mem::write + backtrace (0x313f region) | only pthread sched_param tail-arg writes, no ciphertext |

**Conclusion**: inner ciphertext writes fully bypass touchHLE memory APIs - direct host pointer writes by game native code. touchHLE guest memory is a host Vec; the game binary is pure armv7 JIT. The only explanation: some touchHLE framework implementation (e.g. CFData/NSData bytes write) operates directly on the bytes_mut slice pointer, bypassing mem::write.

---

## 7. Disassembly clues (save chain)

- Save chain: 0x2b1338 (HaXe) -> 0x3795d8 (returns 16) / 0x384bf8 (copies 16B runtime data) -> 0x379494 -> 0x3794bc -> CCCrypt stub 0x3b5b6c
- 0x379494/0x3794bc only pass CCCrypt args; they DO NOT contain inner encryption
- 0x384bf8 copies 16B from runtime data address 0x5aaed64+PC (not static constant); purpose unknown
- key/iv strings: VA 0x51e276 (iv), 0x51e297 (key)
- PSP tables (0x5F67D0/0x5F68D0) exist in iOS binary but do not match inner decryption

---

## 8. Open problems and next-phase suggestions

1. **Inner encryption write path**: intercept at touchHLE bytes_mut/slice-write level, or mprotect + SIGSEGV single-step (guest memory is a host Vec; must confirm page alignment first).
2. **Inner PRNG algorithm**: multi-sample (seed, keystream) differential analysis is the next main attack; need at least 3 full inner-ciphertext captures with different seeds.
3. **Binary reverse-engineering of PRNG function**: in the 1,347,838-line Thumb disassembly, locate by "period-9346 constants 0x2482/0x1241" or "loop writing to caller buffer".
4. **Field mapping**: iOS plaintext vs PSP plaintext structural differences (version header 00010404 vs 00030707) need more controlled samples.
