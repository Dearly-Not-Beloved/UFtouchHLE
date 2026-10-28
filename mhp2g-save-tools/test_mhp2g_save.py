#!/usr/bin/env python3
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
"""Tests for mhp2g_save.py. They use synthetic saves, no game files.

Run with: python3 mhp2g-save-tools/test_mhp2g_save.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mhp2g_save as m  # noqa: E402


def synthetic_plain(platform, name='テスト'):
    plain = bytearray(m.BODY_LEN)
    plain[:m.HEADER_LEN] = platform.default_header
    encoded = name.encode('utf-16-le')
    plain[m.NAME_OFF:m.NAME_OFF + len(encoded)] = encoded
    for i in range(0x2000, 0x2100):
        plain[i] = i & 0xFF
    plain[m.META_OFF:m.HASH_OFF] = bytes(range(12))
    return m.seal(bytes(plain), platform, seed=0x12345678)


class AesTest(unittest.TestCase):
    def test_fips197_vector(self):
        key = bytes(range(16))
        plain = bytes.fromhex('00112233445566778899aabbccddeeff')
        cipher = bytes.fromhex('69c4e0d86a7b0430d8cdb78070b4c55a')
        enc, dec = m._expand_key(key)
        block = m._aes_block(m._block_words(plain, 0), enc, m._TE, m._SBOX, 1)
        self.assertEqual(m._words_block(block), cipher)
        block = m._aes_block(m._block_words(cipher, 0), dec, m._TD,
                             m._INV_SBOX, 3)
        self.assertEqual(m._words_block(block), plain)

    def test_cbc_round_trip(self):
        for n in (0, 1, 15, 16, 17, 100):
            data = bytes(range(n))
            cipher = m.aes_cbc_encrypt(data, m.IOS_AES_KEY, m.IOS_AES_IV)
            self.assertEqual(len(cipher), (n // 16 + 1) * 16)
            self.assertEqual(
                m.aes_cbc_decrypt(cipher, m.IOS_AES_KEY, m.IOS_AES_IV), data)


class GameLayerTest(unittest.TestCase):
    def test_tables_are_permutations(self):
        for platform in m.PLATFORMS.values():
            self.assertEqual(sorted(platform.enc), list(range(256)))
            self.assertEqual(bytes(platform.dec[x] for x in platform.enc),
                             bytes(range(256)))

    def test_round_trip(self):
        for platform in m.PLATFORMS.values():
            plain = synthetic_plain(platform)
            cipher = m.game_encrypt(plain, platform)
            self.assertNotEqual(cipher[:16], plain[:16])
            self.assertEqual(m.game_decrypt(cipher, platform), plain)

    def test_seed_is_stored_through_the_table_twice(self):
        plain = synthetic_plain(m.IOS)
        cipher = m.game_encrypt(plain, m.IOS)
        t = m.IOS.enc
        self.assertEqual(cipher[m.SEED_OFF:],
                         plain[m.SEED_OFF:].translate(t).translate(t))

    def test_zero_seed_halves_use_defaults(self):
        # A seed of 0 behaves like lo = 0x215F, hi = 0xDFA3.
        data = bytes(8)
        self.assertEqual(m._xor_stream(data, 0),
                         m._xor_stream(data, 0xDFA3215F))

    def test_hash_covers_data_but_not_metadata(self):
        plain = bytearray(synthetic_plain(m.IOS))
        self.assertTrue(m.hash_ok(plain, m.IOS))
        self.assertFalse(m.hash_ok(plain, m.PSP))
        plain[m.META_OFF] ^= 1
        self.assertTrue(m.hash_ok(plain, m.IOS))
        plain[0x2000] ^= 1
        self.assertFalse(m.hash_ok(plain, m.IOS))


class FormatTest(unittest.TestCase):
    def test_detect_ios(self):
        plain = synthetic_plain(m.IOS)
        data = m.dump(plain, m.IOS)
        self.assertEqual(len(data), m.IOS_FILE_LEN)
        self.assertEqual(m.detect(data), (m.IOS, plain))

    def test_detect_psp(self):
        plain = synthetic_plain(m.PSP)
        data = m.dump(plain, m.PSP)
        self.assertEqual(len(data), m.BODY_LEN)
        self.assertEqual(m.detect(data), (m.PSP, plain))

    def test_detect_plaintext(self):
        plain = synthetic_plain(m.PSP)
        platform, out = m.detect(plain[:m.SEED_OFF])
        self.assertIs(platform, m.PSP)
        self.assertEqual(out[:m.SEED_OFF], plain[:m.SEED_OFF])

    def test_detect_rejects_corruption(self):
        data = bytearray(m.dump(synthetic_plain(m.PSP), m.PSP))
        data[0x3000] ^= 1
        with self.assertRaises(m.SaveError):
            m.detect(bytes(data))

    def test_convert_both_ways(self):
        psp = synthetic_plain(m.PSP, '君仔')
        ios = m.convert(psp, m.PSP, m.IOS)
        self.assertEqual(ios[:m.HEADER_LEN], m.IOS_DEFAULT_HEADER)
        self.assertEqual(ios[m.HEADER_LEN:m.HASH_OFF],
                         psp[m.HEADER_LEN:m.HASH_OFF])
        self.assertTrue(m.hash_ok(ios, m.IOS))
        self.assertEqual(m.character_name(ios), '君仔')
        self.assertEqual(m.detect(m.dump(ios, m.IOS)), (m.IOS, ios))

        back = m.convert(ios, m.IOS, m.PSP)
        self.assertEqual(back[:m.HASH_OFF], psp[:m.HASH_OFF])
        self.assertTrue(m.hash_ok(back, m.PSP))

    def test_convert_with_template(self):
        template = bytearray(synthetic_plain(m.IOS))
        template[:m.HEADER_LEN] = bytes(range(m.HEADER_LEN))
        ios = m.convert(synthetic_plain(m.PSP), m.PSP, m.IOS, template)
        self.assertEqual(ios[:m.HEADER_LEN], bytes(range(m.HEADER_LEN)))


if __name__ == '__main__':
    unittest.main()
