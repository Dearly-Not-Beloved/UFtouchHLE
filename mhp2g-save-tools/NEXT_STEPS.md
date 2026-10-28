# NEXT_STEPS.md - MHP2G PSP <-> iOS save converter status

> Updated 2026-10-28 (late). See README.md for usage, the format and what
> was verified. This supersedes the earlier "checksum F" analysis.

## Status: the converter works for PSP -> iOS

Checksum "F" turned out to be the same scheme as on the PSP, and the save
crypto is fully understood from the game's own code:

- `0x322704` (load): game-layer decrypt, then
  `SHA-1(plain[0:0x16A0DC] + "3Nc94Hq1zOLh8d62Sb69")` compared with the 20
  bytes at 0x16A0E8. (It temporarily writes the salt over 0x16A0DC..0x16A0F0
  and hashes 0x16A0F0 bytes, which is the same thing.) The salt is the PSP
  MHFU (ULUS-10391) salt from mhef.
- `0x322868`: the game layer. The seed is the last 4 bytes, put through the
  decryption table twice; the LCG is advanced before each word is used.
- `0x322AF4` (save): writes the hash, a new seed at 0x16A0FC, and encrypts.
- `0x2B0B8C` is a thin wrapper around `CC_SHA1`. Earlier notes said the game
  uses no CC_SHA*; it does. No SHA-1 constants are in the binary because the
  hash is CommonCrypto's.

Why the earlier re-encrypted saves were rejected: the old converter didn't
store the seed in the last 4 bytes (it treated them as data), so the game
derived a different keystream and the hash check failed. The "beta recovered
from 0xDFC00 and run backwards 229120 steps" theory was a misreading of the
same thing; the game reads the seed from the end of the file.

Verified in UFtouchHLE: the original save loads; the same save re-encrypted
with a new seed loads; a PSP save converted to iOS loads and its characters
show up on the character select screen; a save with one byte changed and no
new hash is rejected.

## Remaining work (ranked)

1. PSP system layer (sceChnnlsv/KIRK). Decrypting the k73 `MHP2NDG.BIN`
   with Apollo's `endecrypter` or mhef's `PSPSavedataCipher` and the JP game
   key gives data that fails the SHA-1 check; its `SAVEDATA_PARAMS` byte is
   0x41. Find out why (mode, key, or the file itself), then either document
   a working external tool chain or implement the layer (check licences:
   mhef and PPSSPP are GPL).
2. Check iOS -> PSP output on PPSSPP or a PSP.
3. In-game write-back test: load a converted save, save in game, and check
   the result with `mhp2g_save.py inspect`.
4. Field map of the settings block 0x00..0x24 and anything else that might
   be platform-specific (e.g. PSP-only download quests).
5. touchHLE integration: import/export of PSP saves from the launcher, with
   automatic backups.
