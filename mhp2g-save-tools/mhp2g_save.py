#!/usr/bin/env python3
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
"""MHP2G (Monster Hunter Portable 2nd G) PSP <-> iOS save converter.

Only the Python 3 standard library is needed. See README.md for usage.

Both platforms store the same 0x16A100-byte save body, protected by Capcom's
"game layer":

    0x000000..0x16A0DC  save data (0x00..0x24 holds platform-specific settings)
    0x16A0DC..0x16A0E8  12 bytes of metadata, not covered by the hash
    0x16A0E8..0x16A0FC  SHA-1(data[0x0:0x16A0DC] + salt)
    0x16A0FC..0x16A100  seed of the XOR stream

Game layer, per 32-bit little-endian word w (every word except the seed):
    encrypt: w = ENC[w bytes]; advance LCG; w ^= beta; w = ENC[w bytes]
    decrypt: the same with DEC = ENC^-1
    LCG: lo = lo * 0x215F % 0xFF8F, hi = hi * 0xDFA3 % 0xFFEF,
         beta = lo | hi << 16, starting from the seed (halves of 0 become
         0x215F/0xDFA3); the seed is stored as ENC[ENC[seed bytes]].

Both games use the same byte tables; they differ in the salt. iOS (checked
against the game's code at 0x322704, 0x322868 and 0x322AF4 in the MHP2G iOS
binary) uses the salt of the PSP's Monster Hunter Freedom Unite, and wraps
the result in AES-128-CBC with PKCS#7 padding (via CCCrypt), giving a
0x16A110-byte savedata.dat. A PSP "game layer" file is what is left after
the PSP system (sceChnnlsv/KIRK) layer is removed, e.g. with Apollo's
endecrypter and the game key; this tool doesn't implement that layer.
"""

import argparse
import hashlib
import os
import random
import sys
from array import array

BODY_LEN = 0x16A100
HASHED_LEN = 0x16A0DC
META_OFF = 0x16A0DC
HASH_OFF = 0x16A0E8
SEED_OFF = 0x16A0FC
IOS_FILE_LEN = BODY_LEN + 16
# Plaintext lengths this or other tools write: without hash and seed (mhef),
# without seed, and complete.
PLAIN_LENS = (HASH_OFF, SEED_OFF, BODY_LEN)

# Platform-specific settings at the start of the save. The rest of the first
# 0x1000 bytes is identical in all samples from both platforms.
HEADER_LEN = 0x24
NAME_OFF = 0x1000
NAME_LEN = 0x20

LO_MUL, LO_MOD = 0x215F, 0xFF8F
HI_MUL, HI_MOD = 0xDFA3, 0xFFEF

# Encryption table of both games (VA 0x5F67D0 in the iOS binary; the
# decryption table follows at 0x5F68D0). This is not the table mhef uses.
ENC_TABLE = bytes([
    0x3A, 0x11, 0x72, 0x6E, 0x2F, 0x7A, 0x23, 0xF0,
    0x6B, 0x5C, 0xCA, 0x41, 0x57, 0x5D, 0xB2, 0xA8,
    0x06, 0x5F, 0xC9, 0xD8, 0x17, 0x82, 0xBB, 0xD2,
    0x88, 0x86, 0x27, 0x47, 0xC7, 0x29, 0x5B, 0x04,
    0x8E, 0xAE, 0x5E, 0x33, 0x49, 0xDB, 0xA5, 0x7B,
    0x31, 0xB7, 0xB5, 0x84, 0xF6, 0x76, 0x81, 0xAD,
    0x6F, 0xE0, 0x39, 0x87, 0xC0, 0xAB, 0x68, 0x4D,
    0x05, 0x92, 0x77, 0xFF, 0x99, 0x36, 0x2C, 0x07,
    0x0A, 0x59, 0xFA, 0xBC, 0x54, 0xE1, 0x94, 0x3D,
    0x1F, 0x58, 0xA6, 0xAA, 0x53, 0x2E, 0x89, 0xEB,
    0x9C, 0x98, 0x1B, 0xD6, 0xC1, 0x30, 0x00, 0xF4,
    0xEF, 0x7D, 0xBF, 0xE8, 0x0C, 0xC4, 0xAC, 0xF3,
    0xEA, 0x78, 0xBE, 0xAF, 0xB0, 0x08, 0xA1, 0x90,
    0x60, 0x2A, 0xD4, 0x62, 0x3B, 0xA7, 0xD9, 0xED,
    0x7C, 0xF5, 0xEC, 0xBA, 0x9B, 0x4A, 0x0F, 0x3C,
    0x7F, 0xC6, 0x24, 0x55, 0x01, 0x43, 0x22, 0x8A,
    0xA0, 0x70, 0x65, 0x67, 0x50, 0x1D, 0x96, 0x42,
    0xCE, 0x20, 0x03, 0x12, 0xE3, 0x4E, 0x95, 0x26,
    0xE4, 0xB8, 0xD7, 0x48, 0xD0, 0xC3, 0xB9, 0x80,
    0x18, 0xD5, 0x02, 0x34, 0x21, 0x8B, 0xDF, 0xE9,
    0x61, 0x79, 0x0E, 0x93, 0xA3, 0x63, 0x1A, 0x9D,
    0xB3, 0xCD, 0xE6, 0x14, 0x2D, 0xDC, 0xA4, 0x9E,
    0x28, 0x75, 0xA9, 0x66, 0x91, 0xBD, 0xD3, 0x6A,
    0x6D, 0xFB, 0x5A, 0x8C, 0xE7, 0x64, 0x97, 0x19,
    0x46, 0xFE, 0x32, 0xF2, 0x9F, 0x38, 0x9A, 0x10,
    0x3F, 0x6C, 0xE2, 0xD1, 0x45, 0x15, 0xB4, 0xDE,
    0x40, 0x56, 0xE5, 0x3E, 0xF9, 0xC8, 0x35, 0x16,
    0x69, 0xFC, 0x85, 0x8F, 0x51, 0xA2, 0xC5, 0xEE,
    0xB6, 0x8D, 0x74, 0xCC, 0xB1, 0x4F, 0x25, 0xFD,
    0x7E, 0x1C, 0x09, 0x13, 0x4B, 0x37, 0xCF, 0xDA,
    0x4C, 0xF7, 0xC2, 0x52, 0x2B, 0xF1, 0x1E, 0xF8,
    0x0B, 0x83, 0xCB, 0x44, 0x0D, 0x73, 0x71, 0xDD,
])

IOS_SALT = b'3Nc94Hq1zOLh8d62Sb69'
PSP_SALT = b'S)R?Bf8xW3#5h9lGU8wR'

# AES-128-CBC key and IV that the iOS game passes to CCCrypt.
IOS_AES_KEY = b'ls6([f$]=~I(nf28'
IOS_AES_IV = b';lknc(;f_wnX98<i'

# Settings block (0x00..0x24) of every save sample seen from each platform,
# used when a save is converted without a template.
IOS_DEFAULT_HEADER = bytes.fromhex(
    '00010404 00000000 00000001 d9000000 ff7b0118'
    ' f0ffcf03 00000000 00080100 ff39fc07')
PSP_DEFAULT_HEADER = bytes.fromhex(
    '00030707 00000000 00000001 01010000') + b'\xff' * 20


def invert(table):
    inv = bytearray(256)
    for i, v in enumerate(table):
        inv[v] = i
    return bytes(inv)


class Platform:
    def __init__(self, name, enc, salt, default_header):
        assert len(default_header) == HEADER_LEN
        self.name = name
        self.enc = enc
        self.dec = invert(enc)
        self.salt = salt
        self.default_header = default_header


IOS = Platform('ios', ENC_TABLE, IOS_SALT, IOS_DEFAULT_HEADER)
PSP = Platform('psp', ENC_TABLE, PSP_SALT, PSP_DEFAULT_HEADER)
PLATFORMS = {'ios': IOS, 'psp': PSP}


class SaveError(Exception):
    pass


# --- Capcom game layer -------------------------------------------------------

def _xor_stream(data, seed):
    lo = seed & 0xFFFF or LO_MUL
    hi = seed >> 16 or HI_MUL
    words = array('I')
    assert words.itemsize == 4
    words.frombytes(data)
    if sys.byteorder == 'big':
        words.byteswap()
    for i in range(len(words)):
        lo = lo * LO_MUL % LO_MOD
        hi = hi * HI_MUL % HI_MOD
        words[i] ^= lo | hi << 16
    if sys.byteorder == 'big':
        words.byteswap()
    return words.tobytes()


def seed_of(plain):
    return int.from_bytes(plain[SEED_OFF:BODY_LEN], 'little')


def game_decrypt(cipher, platform):
    """Game-layer ciphertext -> plaintext (with the plain seed at the end)."""
    if len(cipher) != BODY_LEN:
        raise SaveError('game-layer data must be 0x%X bytes, not 0x%X'
                        % (BODY_LEN, len(cipher)))
    t = platform.dec
    seed_bytes = cipher[SEED_OFF:].translate(t).translate(t)
    seed = int.from_bytes(seed_bytes, 'little')
    body = _xor_stream(cipher[:SEED_OFF].translate(t), seed).translate(t)
    return body + seed_bytes


def game_encrypt(plain, platform):
    """Plaintext (with the plain seed at the end) -> game-layer ciphertext."""
    assert len(plain) == BODY_LEN
    t = platform.enc
    body = _xor_stream(plain[:SEED_OFF].translate(t), seed_of(plain))
    return body.translate(t) + plain[SEED_OFF:].translate(t).translate(t)


def save_hash(plain, platform):
    return hashlib.sha1(plain[:HASHED_LEN] + platform.salt).digest()


def hash_ok(plain, platform):
    return save_hash(plain, platform) == plain[HASH_OFF:SEED_OFF]


def random_seed():
    # Halves of 0 would be replaced by the defaults; avoid them.
    return random.randrange(1, LO_MOD) | random.randrange(1, HI_MOD) << 16


def seal(plain, platform, seed=None):
    """Store the hash and a new seed, like the game does when saving."""
    out = bytearray(plain)
    out[HASH_OFF:SEED_OFF] = save_hash(out, platform)
    if seed is None:
        seed = random_seed()
    out[SEED_OFF:BODY_LEN] = seed.to_bytes(4, 'little')
    return bytes(out)


# --- AES-128 (FIPS-197), for the iOS outer layer -----------------------------

def _gf_mul(a, b):
    r = 0
    while b:
        if b & 1:
            r ^= a
        a = ((a << 1) ^ (0x1B if a & 0x80 else 0)) & 0xFF
        b >>= 1
    return r


def _make_sbox():
    sbox = bytearray(256)
    sbox[0] = 0x63
    p = q = 1
    while True:
        # Multiply p by 3 and divide q by 3, so that q stays p's inverse.
        p ^= ((p << 1) ^ (0x1B if p & 0x80 else 0)) & 0xFF
        q ^= q << 1
        q ^= q << 2
        q ^= q << 4
        q &= 0xFF
        if q & 0x80:
            q ^= 0x09
        x = q
        for n in range(1, 5):
            x ^= ((q << n) | (q >> (8 - n))) & 0xFF
        sbox[p] = x ^ 0x63
        if p == 1:
            return bytes(sbox)


def _rotations(table):
    return [[((w >> (8 * n)) | (w << (32 - 8 * n))) & 0xFFFFFFFF
             for w in table] for n in range(4)]


_SBOX = _make_sbox()
_INV_SBOX = invert(_SBOX)
_TE = _rotations([_gf_mul(s, 2) << 24 | s << 16 | s << 8 | _gf_mul(s, 3)
                  for s in _SBOX])
_TD = _rotations([_gf_mul(s, 14) << 24 | _gf_mul(s, 9) << 16
                  | _gf_mul(s, 13) << 8 | _gf_mul(s, 11) for s in _INV_SBOX])


def _expand_key(key):
    """Round keys for encryption and for the equivalent inverse cipher."""
    w = [int.from_bytes(key[i:i + 4], 'big') for i in range(0, 16, 4)]
    rcon = 1
    for i in range(4, 44):
        t = w[i - 1]
        if i % 4 == 0:
            t = ((t << 8) | (t >> 24)) & 0xFFFFFFFF
            t = (_SBOX[t >> 24] << 24 | _SBOX[t >> 16 & 0xFF] << 16
                 | _SBOX[t >> 8 & 0xFF] << 8 | _SBOX[t & 0xFF]) ^ rcon << 24
            rcon = _gf_mul(rcon, 2)
        w.append(w[i - 4] ^ t)
    enc = [w[r * 4:r * 4 + 4] for r in range(11)]

    def inv_mix_columns(x):
        return (_TD[0][_SBOX[x >> 24]] ^ _TD[1][_SBOX[x >> 16 & 0xFF]]
                ^ _TD[2][_SBOX[x >> 8 & 0xFF]] ^ _TD[3][_SBOX[x & 0xFF]])
    dec = ([enc[10]] + [[inv_mix_columns(x) for x in enc[r]]
                        for r in range(9, 0, -1)] + [enc[0]])
    return enc, dec


def _aes_block(s, round_keys, t, box, step):
    # step is 1 for encryption (ShiftRows) and 3 for decryption
    # (InvShiftRows): column i takes row r from column i + r * step.
    rk = round_keys[0]
    s = [s[i] ^ rk[i] for i in range(4)]
    t0, t1, t2, t3 = t
    for rk in round_keys[1:10]:
        s = [t0[s[i] >> 24] ^ t1[s[(i + step) % 4] >> 16 & 0xFF]
             ^ t2[s[(i + 2 * step) % 4] >> 8 & 0xFF]
             ^ t3[s[(i + 3 * step) % 4] & 0xFF] ^ rk[i] for i in range(4)]
    rk = round_keys[10]
    return [(box[s[i] >> 24] << 24 | box[s[(i + step) % 4] >> 16 & 0xFF] << 16
             | box[s[(i + 2 * step) % 4] >> 8 & 0xFF] << 8
             | box[s[(i + 3 * step) % 4] & 0xFF]) ^ rk[i] for i in range(4)]


def _block_words(data, i):
    return [int.from_bytes(data[j:j + 4], 'big') for j in range(i, i + 16, 4)]


def _words_block(words):
    return b''.join(x.to_bytes(4, 'big') for x in words)


def aes_cbc_encrypt(data, key, iv):
    pad = 16 - len(data) % 16
    data = data + bytes([pad]) * pad
    enc, _ = _expand_key(key)
    prev = _block_words(iv, 0)
    out = []
    for i in range(0, len(data), 16):
        block = [x ^ y for x, y in zip(_block_words(data, i), prev)]
        prev = _aes_block(block, enc, _TE, _SBOX, 1)
        out.append(_words_block(prev))
    return b''.join(out)


def aes_cbc_decrypt(data, key, iv):
    if len(data) % 16 or not data:
        raise SaveError('AES data length is not a multiple of 16')
    _, dec = _expand_key(key)
    prev = _block_words(iv, 0)
    out = []
    for i in range(0, len(data), 16):
        block = _block_words(data, i)
        plain = _aes_block(block, dec, _TD, _INV_SBOX, 3)
        out.append(_words_block([x ^ y for x, y in zip(plain, prev)]))
        prev = block
    out = b''.join(out)
    pad = out[-1]
    if not 1 <= pad <= 16 or out[-pad:] != bytes([pad]) * pad:
        raise SaveError('bad PKCS#7 padding')
    return out[:-pad]


# --- File formats ------------------------------------------------------------

def load_ios(raw):
    if len(raw) != IOS_FILE_LEN:
        raise SaveError('an iOS savedata.dat is 0x%X bytes, not 0x%X'
                        % (IOS_FILE_LEN, len(raw)))
    return game_decrypt(aes_cbc_decrypt(raw, IOS_AES_KEY, IOS_AES_IV), IOS)


def dump(plain, platform):
    """Plaintext -> save file of the platform."""
    data = game_encrypt(plain, platform)
    if platform is IOS:
        data = aes_cbc_encrypt(data, IOS_AES_KEY, IOS_AES_IV)
    return data


def complete_plain(raw, platform):
    """Pad a plaintext from this or another tool to 0x16A100 bytes."""
    if len(raw) not in PLAIN_LENS:
        raise SaveError('a plaintext save is 0x%X, 0x%X or 0x%X bytes, not '
                        '0x%X' % (PLAIN_LENS + (len(raw),)))
    return seal(raw + bytes(BODY_LEN - len(raw)), platform)


def detect(raw):
    """Return (platform, plaintext) for a save file of either platform."""
    errors = []
    if len(raw) == IOS_FILE_LEN:
        try:
            plain = load_ios(raw)
            if hash_ok(plain, IOS):
                return IOS, plain
            errors.append('iOS: decrypted, but the SHA-1 does not match')
        except SaveError as e:
            errors.append('iOS: %s' % e)
        errors.append('PSP: this may be a PSP save that still has its system '
                      '(sceChnnlsv) encryption; remove it first (README.md)')
    if len(raw) == BODY_LEN:
        plain = game_decrypt(raw, PSP)
        if hash_ok(plain, PSP):
            return PSP, plain
        errors.append('PSP game layer: the SHA-1 does not match')
    if len(raw) in (SEED_OFF, BODY_LEN):
        for platform in (IOS, PSP):
            if hash_ok(raw, platform):
                return platform, complete_plain(raw, platform)
        errors.append('plaintext: the SHA-1 matches neither platform')
    if not errors:
        errors.append('the size (0x%X) matches no known format' % len(raw))
    raise SaveError('unrecognized save file:\n  ' + '\n  '.join(errors))


def character_name(plain):
    raw = plain[NAME_OFF:NAME_OFF + NAME_LEN]
    for i in range(0, len(raw), 2):
        if raw[i:i + 2] == b'\0\0':
            raw = raw[:i]
            break
    return raw.decode('utf-16-le', 'replace')


def convert(plain, src, dst, template=None):
    """Move a plaintext save from platform src to platform dst."""
    out = bytearray(plain)
    if src is not dst:
        header = dst.default_header if template is None else template
        out[:HEADER_LEN] = header[:HEADER_LEN]
    return seal(out, dst)


# --- Command line ------------------------------------------------------------

def read(path):
    with open(path, 'rb') as f:
        return f.read()


def write(path, data, force):
    if os.path.exists(path) and not force:
        raise SaveError('%s exists; pass --force to overwrite it' % path)
    with open(path, 'wb') as f:
        f.write(data)


def cmd_inspect(args):
    platform, plain = detect(read(args.input))
    print('platform: %s' % platform.name)
    print('name:     %s' % character_name(plain))
    print('settings: %s' % plain[:HEADER_LEN].hex(' '))
    print('metadata: %s' % plain[META_OFF:HASH_OFF].hex(' '))
    print('sha1:     %s (ok)' % plain[HASH_OFF:SEED_OFF].hex())
    print('seed:     0x%08X' % seed_of(plain))


def cmd_convert(args):
    src, plain = detect(read(args.input))
    dst = PLATFORMS[args.to]
    template = None
    if args.template:
        template_platform, template = detect(read(args.template))
        if template_platform is not dst:
            raise SaveError('the template must be a %s save' % dst.name)
    out = convert(plain, src, dst, template)
    data = dump(out, dst)
    # Read the result back like the game would before writing it.
    assert detect(data) == (dst, out)
    write(args.output, data, args.force)
    print('%s -> %s: %s (%s)' % (src.name, dst.name, args.output,
                                 character_name(out)))


def cmd_decrypt(args):
    platform, plain = detect(read(args.input))
    write(args.output, plain, args.force)
    print('%s plaintext: %s' % (platform.name, args.output))


def cmd_encrypt(args):
    platform = PLATFORMS[args.platform]
    plain = complete_plain(read(args.input), platform)
    write(args.output, dump(plain, platform), args.force)
    print('%s save: %s' % (platform.name, args.output))


def main(argv=None):
    ap = argparse.ArgumentParser(
        description='MHP2G PSP <-> iOS save converter')
    sub = ap.add_subparsers(dest='command', required=True)

    p = sub.add_parser('inspect', help='identify and check a save')
    p.add_argument('input')
    p.set_defaults(func=cmd_inspect)

    p = sub.add_parser('convert', help='convert a save to another platform')
    p.add_argument('input', help='iOS savedata.dat, PSP game-layer save, '
                   'or plaintext')
    p.add_argument('output')
    p.add_argument('--to', choices=PLATFORMS, required=True)
    p.add_argument('--template', help='a save of the target platform to '
                   'take the settings block (0x00-0x24) from')
    p.add_argument('--force', action='store_true',
                   help='overwrite the output file')
    p.set_defaults(func=cmd_convert)

    p = sub.add_parser('decrypt', help='write the 0x16A100-byte plaintext')
    p.add_argument('input')
    p.add_argument('output')
    p.add_argument('--force', action='store_true',
                   help='overwrite the output file')
    p.set_defaults(func=cmd_decrypt)

    p = sub.add_parser('encrypt',
                       help='encrypt a plaintext, updating its hash and seed')
    p.add_argument('input')
    p.add_argument('output')
    p.add_argument('--platform', choices=PLATFORMS, required=True)
    p.add_argument('--force', action='store_true',
                   help='overwrite the output file')
    p.set_defaults(func=cmd_encrypt)

    args = ap.parse_args(argv)
    try:
        args.func(args)
    except (SaveError, OSError) as e:
        print('error: %s' % e, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
