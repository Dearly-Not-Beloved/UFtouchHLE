#!/usr/bin/env python3
"""MHP2G PSP <-> iOS 存档转换器 v0.1
破解结论（已验证）：
- iOS 外层: AES-128-CBC, key=ls6([f$]=~I(nf28, iv=;lknc(;f_wnX98<if, PKCS7
- iOS 内层: 表置换 c=T1[T1[p]^beta], T1@0x5f27d0/T2@0x5f28d0 (与PSP同表)
  beta = lower + (upper<<16), lower=(lower*0x215F)%0xFF8F, upper=(upper*0xDFA3)%0xFFEF
- PSP 系统层: KIRK AES-128-CBC (mhef), 游戏层同表+同LCG, seed=尾4B经表2置换2次
"""
import struct, sys, os, hashlib, argparse

BIN_PATH = '/tmp/mhp2g_ios/Payload/MHP2G.app/MHP2G'
T1_OFF, T2_OFF = 0x5f27d0, 0x5f28d0
A_LO, M_LO = 0x215F, 0xFF8F
A_HI, M_HI = 0xDFA3, 0xFFEF
INNER_LEN = 0x16A100

# PSP 游戏层
PSP_KEY = bytes.fromhex('cd1f2059ae70ef68dca24513b45adb0a')

PSP_TABLE = bytes([0xA6,0x21,0x22,0xF9,0x3D,0xFB,0xF5,0x06,0x87,0x54,0x76,0x75,0xED,0x16,0x33,0x2E,
0x5A,0x17,0x50,0x1C,0xDE,0xAF,0x73,0x39,0xB5,0x28,0xD9,0xF1,0xCD,0x98,0x5F,0x2D,
0x78,0x62,0x29,0xC9,0xFD,0xEA,0x32,0xCC,0x31,0x70,0x34,0x61,0xAE,0x4D,0xFE,0xC2,
0x45,0x24,0xF8,0xFC,0xD7,0x2F,0xD8,0x26,0x59,0xA0,0xBD,0xA5,0x01,0x18,0xA1,0x95,
0xEE,0x4B,0x1A,0x7A,0x5B,0xDB,0xF0,0x27,0xE6,0xF4,0xB2,0xAD,0x4A,0x14,0x9A,0x20,
0xB9,0x36,0x4F,0x3B,0xCA,0xE4,0x41,0x85,0x3A,0x46,0x5C,0xCB,0x3C,0xEC,0x63,0x84,
0x66,0xC4,0xEB,0x25,0xAA,0x7E,0xC3,0x47,0x9B,0x74,0x8B,0x5D,0x23,0x8F,0x72,0x81,
0x8C,0x56,0xC6,0xD4,0x40,0x60,0xA2,0x6D,0xE5,0xB0,0x15,0x58,0x52,0x0F,0x7D,0x67,
0xEF,0xD2,0x0B,0xCF,0x0D,0xD6,0x2B,0x0A,0x9F,0x80,0x5E,0x3F,0x71,0x68,0x05,0x86,
0xBB,0x38,0xCE,0x7F,0xF3,0x83,0x03,0x48,0xD3,0xFA,0x35,0xDF,0x44,0x7C,0x82,0x93,
0x53,0xA3,0x09,0x6A,0x77,0x13,0x55,0xDD,0xBE,0x00,0x9C,0xB4,0xE3,0x9E,0xBC,0x96,
0xA7,0xE2,0x42,0x4E,0x37,0x0E,0xF6,0x9D,0x1B,0x2A,0x79,0xBF,0xDC,0xA9,0x88,0x4C,
0xD0,0xAB,0xC5,0x69,0xD1,0xE1,0xAC,0xE9,0xC1,0xB6,0x7B,0x57,0x90,0x07,0x30,0x92,
0xB7,0x1E,0xDA,0x49,0x0C,0xB8,0xA4,0xFF,0xB3,0xF2,0x64,0x8A,0xC7,0x02,0x6C,0x08,
0x6F,0xF7,0x89,0xC0,0x10,0xA8,0x8D,0xBA,0x91,0x43,0x6B,0x3E,0xD5,0x65,0xE0,0x6E,
0x12,0x2C,0x94,0x99,0xE8,0x1F,0xC8,0x11,0x8E,0xB1,0xE7,0x51,0x1D,0x97,0x04,0x19])

PSP_SALT = b'S)R?Bf8xW3#5h9lGU8wR'
PSP_LCG_A_LO, PSP_LCG_M_LO = 0x215F, 0xFF8F
PSP_LCG_A_HI, PSP_LCG_M_HI = 0xDFA3, 0xFFEF

_t1 = _t2 = None
def tables():
    global _t1, _t2
    if _t1 is None:
        d = open(BIN_PATH,'rb').read()
        _t1 = d[T1_OFF:T1_OFF+256]
        _t2 = d[T2_OFF:T2_OFF+256]
    return _t1, _t2

def recover_beta0(cipher, blk=100):
    t1, t2 = tables()
    c = cipher[blk*4:(blk+1)*4]
    b = struct.unpack('<I', bytes(t2[x] ^ t1[0] for x in c))[0]
    lo, hi = b & 0xFFFF, b >> 16
    ilo = pow(A_LO, -1, M_LO); ihi = pow(A_HI, -1, M_HI)
    for _ in range(blk):
        lo = lo * ilo % M_LO; hi = hi * ihi % M_HI
    return lo, hi

def beta_stream(lo0, hi0, n):
    lo, hi = lo0, hi0
    for _ in range(n):
        yield lo | (hi << 16)
        lo = lo * A_LO % M_LO; hi = hi * A_HI % M_HI

def ios_inner_decrypt(inner):
    t1, t2 = tables()
    n = len(inner) // 4
    lo0, hi0 = recover_beta0(inner)
    out = bytearray(n*4)
    for i, beta in enumerate(beta_stream(lo0, hi0, n)):
        c = inner[i*4:(i+1)*4]
        out[i*4:(i+1)*4] = bytes(t2[t2[c[j]] ^ ((beta >> (8*j)) & 0xFF)] for j in range(4))
    return bytes(out), lo0, hi0

def ios_inner_encrypt(plain, lo0, hi0):
    t1, t2 = tables()
    n = len(plain) // 4
    out = bytearray(n*4)
    for i, beta in enumerate(beta_stream(lo0, hi0, n)):
        p = plain[i*4:(i+1)*4]
        out[i*4:(i+1)*4] = bytes(t1[t1[p[j]] ^ ((beta >> (8*j)) & 0xFF)] for j in range(4))
    return bytes(out)

def ios_aes_decrypt(outer):
    from Crypto.Cipher import AES
    key = b'ls6([f$]=~I(nf28'
    iv = bytes.fromhex('3b6c6b6e63283b665f776e5839383c69')
    c = AES.new(key, AES.MODE_CBC, iv)
    inner = c.decrypt(outer)
    # PKCS7 去填充
    pad = inner[-1]
    if 1 <= pad <= 16 and inner[-pad:] == bytes([pad])*pad:
        inner = inner[:-pad]
    return inner

def ios_aes_encrypt(inner):
    from Crypto.Cipher import AES
    key = b'ls6([f$]=~I(nf28'
    iv = bytes.fromhex('3b6c6b6e63283b665f776e5839383c69')
    pad = 16 - (len(inner) % 16)
    inner = inner + bytes([pad])*pad
    c = AES.new(key, AES.MODE_CBC, iv)
    return c.encrypt(inner)

def ios_load(path):
    """读 iOS 存档: 外层AES解密 -> 内层表置换解密 -> 明文"""
    outer = open(path,'rb').read()
    inner = ios_aes_decrypt(outer)
    plain, lo0, hi0 = ios_inner_decrypt(inner)
    return plain, lo0, hi0, inner

def ios_save(plain, lo0, hi0):
    """明文 -> 内层加密 -> 外层AES加密 -> 存档"""
    inner = ios_inner_encrypt(plain, lo0, hi0)
    return ios_aes_encrypt(inner)

def psp_decrypt_system(binfile):
    """PSP 系统层(KIRK AES) 解密 —— 用 mhef"""
    import subprocess
    env = dict(os.environ, PYTHONPATH='/tmp/mhef')
    r = subprocess.run(['python3','/tmp/psp_decrypt.py', binfile, '/tmp/_psp_layer1.bin'],
                       capture_output=True, text=True, env=env)
    if r.returncode != 0:
        raise RuntimeError(f'psp system decrypt failed: {r.stderr}')
    return open('/tmp/_psp_layer1.bin','rb').read()

def psp_game_decrypt(layer1):
    """PSP 游戏层解密 (psp_decrypt.py 算法, 已验证):
    seed=尾4B经表置换2次; p = TABLE[TABLE[c]] ^ beta"""
    T = PSP_TABLE
    data = bytearray(layer1)
    ln = len(data) - 4
    seed = bytes(data[ln:ln+4])
    seed = bytes(T[b] for b in seed)
    seed = bytes(T[b] for b in seed)
    alpha = struct.unpack('<I', seed)[0]
    lower = alpha & 0xFFFF; upper = (alpha >> 16) & 0xFFFF
    if lower == 0: lower = PSP_LCG_A_LO
    if upper == 0: upper = PSP_LCG_A_HI
    for i in range(0, ln, 4):
        for j in range(4): data[i+j] = T[data[i+j]]
        a = data[i] | (data[i+1]<<8) | (data[i+2]<<16) | (data[i+3]<<24)
        lower = (lower * PSP_LCG_A_LO) % PSP_LCG_M_LO
        upper = (upper * PSP_LCG_A_HI) % PSP_LCG_M_HI
        beta = lower + (upper << 16)
        gamma = (a ^ beta) & 0xFFFFFFFF
        data[i] = gamma & 0xFF; data[i+1] = (gamma>>8)&0xFF
        data[i+2] = (gamma>>16)&0xFF; data[i+3] = (gamma>>24)&0xFF
        for j in range(4): data[i+j] = T[data[i+j]]
    data[ln:ln+4] = seed
    return bytes(data[:ln]), alpha

def psp_game_encrypt(plain):
    """PSP 游戏层加密: p = TABLE[TABLE[c]]^beta 的逆运算
    c = IT[IT[p ^ beta]] (IT=逆表), 尾4B 写 seed"""
    T = PSP_TABLE
    IT = bytes([0]*256)
    for i, v in enumerate(T): IT[v] = i
    import random
    lo0 = random.randrange(1, PSP_LCG_M_LO); hi0 = random.randrange(1, PSP_LCG_M_HI)
    n = len(plain) // 4
    out = bytearray(n*4)
    lo, hi = lo0, hi0
    for i in range(n):
        lower = (lo * PSP_LCG_A_LO) % PSP_LCG_M_LO
        upper = (hi * PSP_LCG_A_HI) % PSP_LCG_M_HI
        beta = lower + (upper << 16)
        p = plain[i*4:(i+1)*4]
        for j in range(4):
            x = p[j] ^ ((beta >> (8*j)) & 0xFF)
            out[i*4+j] = IT[IT[x]]
        lo, hi = lower, upper
    # seed: 用 lo0/hi0 经 TABLE 置换2次
    seed = struct.pack('<H', lo0) + struct.pack('<H', hi0)
    seed = bytes(T[b] for b in seed)
    seed = bytes(T[b] for b in seed)
    out += seed
    return bytes(out)

def inspect(plain, platform):
    """显示角色基本信息"""
    name_raw = plain[0x1000:0x1020]
    try:
        name = name_raw.decode('utf-16le').rstrip('\x00')
    except: name = repr(name_raw)
    hdr = plain[:4].hex(' ')
    gender = '男' if platform=='psp' else '?'
    print(f'  平台: {platform}')
    print(f'  版本头: {hdr}')
    print(f'  角色名: {name}')
    print(f'  明文长度: {len(plain)}')
    return {'name': name, 'hdr': hdr}

def convert_psp_to_ios(psp_plain, ios_template_plain):
    """PSP 明文 -> iOS 明文: 保留 iOS 模板头(0x0-0x1000)和尾(0x16a0d4+), 主体拷贝"""
    out = bytearray(ios_template_plain)
    src_start, src_end = 0x1000, 0x16a0d4
    out[src_start:src_end] = psp_plain[src_start:src_end]
    return bytes(out)

def convert_ios_to_psp(ios_plain, psp_template_plain):
    """iOS 明文 -> PSP 明文: 保留 PSP 头尾, 主体拷贝"""
    out = bytearray(psp_template_plain)
    src_start, src_end = 0x1000, 0x16a0d4
    out[src_start:src_end] = ios_plain[src_start:src_end]
    return bytes(out)

if __name__ == '__main__':
    ap = argparse.ArgumentParser(description='MHP2G save converter v0.1')
    sub = ap.add_subparsers(dest='cmd')
    p_i = sub.add_parser('inspect')
    p_i.add_argument('path'); p_i.add_argument('--platform', choices=['auto','psp','ios'], default='auto')
    p_c = sub.add_parser('convert')
    p_c.add_argument('--from', dest='frm', required=True)
    p_c.add_argument('--to', dest='to', required=True)
    p_c.add_argument('--input', required=True)
    p_c.add_argument('--output', required=True)
    args = ap.parse_args()
    if args.cmd == 'inspect':
        plat = args.platform
        if plat == 'auto':
            d = open(args.path,'rb').read()
            plat = 'ios' if len(d) == 1483024 else 'psp'
        if plat == 'ios':
            plain,_,_,_ = ios_load(args.path)
        else:
            layer1 = psp_decrypt_system(args.path)
            plain, seed = psp_game_decrypt(layer1)
        inspect(plain, plat)
    elif args.cmd == 'convert':
        if args.frm == 'psp' and args.to == 'ios':
            layer1 = psp_decrypt_system(args.input)
            psp_plain, seed = psp_game_decrypt(layer1)
            # iOS 模板: 用男档明文
            tpl = open('/tmp/mhp2g_analysis/user_samples/male.plain.bin','rb').read()
            ios_plain = convert_psp_to_ios(psp_plain, tpl)
            # 加密写档
            import random
            lo0 = random.randrange(1, M_LO); hi0 = random.randrange(1, M_HI)
            outer = ios_save(ios_plain, lo0, hi0)
            open(args.output,'wb').write(outer)
            print(f'PSP->iOS 转换完成: {args.output}')
            inspect(ios_plain, 'ios')
        elif args.frm == 'ios' and args.to == 'psp':
            plain,_,_,_ = ios_load(args.input)
            tpl = open('/tmp/MHP2NDG.t2.plain','rb').read()
            psp_plain = convert_ios_to_psp(plain, tpl)
            enc = psp_game_encrypt(psp_plain)
            open(args.output,'wb').write(enc)
            print(f'iOS->PSP 转换完成: {args.output}')
            inspect(psp_plain, 'psp')
        else:
            print('仅支持 psp->ios 或 ios->psp')
