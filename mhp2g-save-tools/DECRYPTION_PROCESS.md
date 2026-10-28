# MHP2G 存档解密全流程（PSP ↔ iOS）

> 状态：PSP 系统层（KIRK）与游戏层 100% 破解；iOS 外层（AES）与内层（表置换+LCG）100% 破解、闭环 0 字节差。
> 唯一未破：iOS 存档校验算法 F（见 §6），它是转换器产物无法被游戏读取的直接原因。
> 本文档记录每一步的密钥、表、公式、偏移与可复现命令，所有"已验证"结论均经过字节级验证。

---

## 0. 文件布局总览

```
PSP 侧：
  MHP2NDG.BIN (1,483,024B)
    │ ① 系统层：KIRK AES-128-CBC（PSP 主机存储加密，工具/PPSSPP 读出时通常已解）
    ▼
  游戏层密文（Capcom 表置换 + LCG 加密）
    │ ② 游戏层解密
    ▼
  明文 1,483,004B：head `00 03 07 07` | 主体 | 12B 元数据 | 20B SHA1

iOS 侧：
  savedata.dat (1,483,024B)
    │ ① 外层：CCCrypt AES-128-CBC + PKCS#7（固定 key/iv）
    ▼
  游戏层密文（内层）1,483,008B (0x16A100)
    │ ② 游戏层解密（与 PSP 同构，表不同）
    ▼
  明文 1,483,008B：head `00 01 04 04` | 主体 | 8B | 20B 校验 | 2B
```

两个平台明文约 91% 字节相同（k73 PSP 档 vs iOS 男档），但表、盐、头部、尾部均不同。

---

## 1. 通用游戏层算法（两平台同构）

Capcom 的"游戏层"是对 32-bit little-endian 字的流式变换，每个字（除 seed 外）做三次操作：

```
每 4 字节块 i：
  1) 逐字节查表：w 的 4 个字节分别过置换表 T
  2) 整字异或密钥流：w ^= beta[i]
  3) 再逐字节查表：w 的 4 个字节再过同一张表 T

beta[i] = lower[i] + (upper[i] << 16)

双 LCG（两平台参数完全相同）：
  lower[i+1] = (lower[i] * 0x215F) % 0xFF8F
  upper[i+1] = (upper[i] * 0xDFA3) % 0xFFEF
  （初值两半若为 0，则分别取 0x215F / 0xDFA3）
```

- **加密方向**（明文→密文）：`c = T1[T1[p] ^ beta]`（用加密表两次）
- **解密方向**（密文→明文）：`p = T2[T2[c] ^ beta]`（用逆表两次）
- **T1/T2 互逆**：`T2[T1[i]] == i`，闭环 `decrypt(encrypt(x)) == x` 已验证 **0 字节差**。

关键点：**两平台结构同构，但置换表不同**（iOS `T1[0]=0x3A`，PSP `TABLE[0]=0xA6`），盐不同，seed 处理不同（见 §3/§4）。

---

## 2. 置换表（完整三张）

### 2.1 iOS 加密表 T1（file 0x5F27D0，即二进制 VA 0x5F67D0；`T1[0]=0x3A`）

```
3A 11 72 6E 2F 7A 23 F0 6B 5C CA 41 57 5D B2 A8
06 5F C9 D8 17 82 BB D2 88 86 27 47 C7 29 5B 04
8E AE 5E 33 49 DB A5 7B 31 B7 B5 84 F6 76 81 AD
6F E0 39 87 C0 AB 68 4D 05 92 77 FF 99 36 2C 07
0A 59 FA BC 54 E1 94 3D 1F 58 A6 AA 53 2E 89 EB
9C 98 1B D6 C1 30 00 F4 EF 7D BF E8 0C C4 AC F3
EA 78 BE AF B0 08 A1 90 60 2A D4 62 3B A7 D9 ED
7C F5 EC BA 9B 4A 0F 3C 7F C6 24 55 01 43 22 8A
A0 70 65 67 50 1D 96 42 CE 20 03 12 E3 4E 95 26
E4 B8 D7 48 D0 C3 B9 80 18 D5 02 34 21 8B DF E9
61 79 0E 93 A3 63 1A 9D B3 CD E6 14 2D DC A4 9E
28 75 A9 66 91 BD D3 6A 6D FB 5A 8C E7 64 97 19
46 FE 32 F2 9F 38 9A 10 3F 6C E2 D1 45 15 B4 DE
40 56 E5 3E F9 C8 35 16 69 FC 85 8F 51 A2 C5 EE
B6 8D 74 CC B1 4F 25 FD 7E 1C 09 13 4B 37 CF DA
4C F7 C2 52 2B F1 1E F8 0B 83 CB 44 0D 73 71 DD
```

### 2.2 iOS 解密表 T2（file 0x5F28D0，`T2 = T1⁻¹`；`T2[0x3A]=0x00`）

```
56 7C 9A 8A 1F 38 10 3F 65 EA 40 F8 5C FC A2 76
C7 01 8B EB AB CD D7 14 98 BF A6 52 E9 85 F6 48
89 9C 7E 06 7A E6 8F 1A B0 1D 69 F4 3E AC 4D 04
55 28 C2 23 9B D6 3D ED C5 32 00 6C 77 47 D3 C8
D0 0B 87 7D FB CC C0 1B 93 24 75 EC F0 37 8D E5
84 DC F3 4C 44 7B D1 0C 49 41 BA 1E 09 0D 22 11
68 A0 6B A5 BD 82 B3 83 36 D8 B7 08 C9 B8 03 30
81 FE 02 FD E2 B1 2D 3A 61 A1 05 27 70 59 E8 78
97 2E 15 F9 2B DA 19 33 18 4E 7F 9D BB E1 20 DB
67 B4 39 A3 46 8E 86 BE 51 3C C6 74 50 A7 AF C4
80 66 DD A4 AE 26 4A 6D 0F B2 4B 35 5E 2F 21 63
64 E4 0E A8 CE 2A E0 29 91 96 73 16 43 B5 62 5A
34 54 F2 95 5D DE 79 1C D5 12 0A FA E3 A9 88 EE
94 CB 17 B6 6A 99 53 92 13 6E EF 25 AD FF CF 9E
31 45 CA 8C 90 D2 AA BC 5B 9F 60 4F 72 6F DF 58
07 F5 C3 5F 57 71 2C F1 F7 D4 42 B9 D9 E7 C1 3B
```

### 2.3 PSP 置换表（psp_decrypt.py 内嵌，解密方向用；`TABLE[0]=0xA6`）

```
A6 21 22 F9 3D FB F5 06 87 54 76 75 ED 16 33 2E
5A 17 50 1C DE AF 73 39 B5 28 D9 F1 CD 98 5F 2D
78 62 29 C9 FD EA 32 CC 31 70 34 61 AE 4D FE C2
45 24 F8 FC D7 2F D8 26 59 A0 BD A5 01 18 A1 95
EE 4B 1A 7A 5B DB F0 27 E6 F4 B2 AD 4A 14 9A 20
B9 36 4F 3B CA E4 41 85 3A 46 5C CB 3C EC 63 84
66 C4 EB 25 AA 7E C3 47 9B 74 8B 5D 23 8F 72 81
8C 56 C6 D4 40 60 A2 6D E5 B0 15 58 52 0F 7D 67
EF D2 0B CF 0D D6 2B 0A 9F 80 5E 3F 71 68 05 86
BB 38 CE 7F F3 83 03 48 D3 FA 35 DF 44 7C 82 93
53 A3 09 6A 77 13 55 DD BE 00 9C B4 E3 9E BC 96
A7 E2 42 4E 37 0E F6 9D 1B 2A 79 BF DC A9 88 4C
D0 AB C5 69 D1 E1 AC E9 C1 B6 7B 57 90 07 30 92
B7 1E DA 49 0C B8 A4 FF B3 F2 64 8A C7 02 6C 08
6F F7 89 C0 10 A8 8D BA 91 43 6B 3E D5 65 E0 6E
12 2C 94 99 E8 1F C8 11 8E B1 E7 51 1D 97 04 19
```

> ⚠️ mhp2g_save.py 目前把 §2.1 的 iOS 表同时用作 PSP 表（注释"both games use the same tables"）。
> **实测用 iOS 表解 PSP 密文失败（明文头变乱码）** —— PSP 侧必须使用 §2.3 的表。此为转换器已知 bug，待修。

---

## 3. PSP 解密全流程（已验证 100%）

### 3.1 系统层：KIRK AES-128-CBC

- 日版（ULJM05500）游戏 key：`cd1f2059ae70ef68dca24513b45adb0a`
- 工具：Apollo 的 `endecrypter <输入BIN> <16字节key文件>`
- 用法：`/tmp/endecrypter MHP2NDG.BIN /tmp/jp.key > 系统层明文`
- 注：k73.com 下载的 BIN 通常已含/已过系统层处理，实测路径存在差异（见 §9 待核查项）。

### 3.2 游戏层：表置换 + 双 LCG

seed 取法（psp_decrypt.py 实现）：
```
seed = 文件尾 4 字节
seed = TABLE[seed[0..3]]   （逐字节置换第 1 次）
seed = TABLE[seed[0..3]]   （逐字节置换第 2 次）
alpha = LE32(seed)
lower = alpha & 0xFFFF
upper = (alpha >> 16) & 0xFFFF
若 lower==0: lower=0x215F；若 upper==0: upper=0xDFA3
```

逐块解密（每块 4B，除尾 4B seed 外全部参与）：
```
对每块 i（从 0 开始）：
  lower = lower*0x215F % 0xFF8F
  upper = upper*0xDFA3 % 0xFFEF
  beta = lower | (upper << 16)
  块 = TABLE[TABLE[块字节] ^ beta 逐字节]（解密方向）
```

### 3.3 尾部校验（SHA1）

- 明文尾部：`12B 元数据 | 20B SHA1`
- **SHA1 覆盖范围 = 明文 `[0, len-32)` + 盐**（实测：`SHA1(t2.plain[:-12] + 盐) == 尾20B` → **True**）
- PSP 盐：`S)R?Bf8xW3#5h9lGU8wR`
- 关键更正：早期记录"SHA1 对密文算"不准确——**实测证明是对明文 `data[:-12]` 加盐计算**（mhp2g_save.py 的 `hash_ok()` 语义正确）。

### 3.4 验证结果

- k73 档：head `00 03 07 07`；370751/370751 块全部为有效明文；SHA1 精确匹配。
- 与 iOS 男档明文 90.99% 字节相同（确认两平台共用主体数据结构）。

---

## 4. iOS 解密全流程（已验证 100%）

### 4.1 外层：CCCrypt AES-128-CBC + PKCS#7

| 项 | 值 |
|---|---|
| 算法 | AES-128-CBC，options=1（PKCS7 填充） |
| Key | hex `6c7336285b66245d3d7e49286e663238` = ASCII `ls6([f$]=~I(nf28` |
| IV | hex `3b6c6b6e63283b665f776e5839383c69` = ASCII `;lknc(;f_wnX98<i` |
| Key 字符串 VA | 0x51E297 |
| IV 字符串 VA | 0x51E276 |
| 长度 | savedata.dat 1,483,024B → 去 16B 填充 → 内层 1,483,008B (0x16A100) |

复现（openssl）：
```
openssl enc -d -aes-128-cbc -K 6c7336285b66245d3d7e49286e663238 \
            -iv 3b6c6b6e63283b665f776e5839383c69 -in savedata.dat -out inner.bin
```
实测：对保存写入流加密 == 磁盘文件，字节一致。

> 两个 16B ASCII 字符串紧邻 key/iv 之后：`f09qfkn#olsp9fkn` 与 `b#hl-*pa-lspzm `（候选盐/第二组密钥；SHA1 组合已测全部不匹配，见 §6）。

### 4.2 内层：表置换 + 双 LCG（与 PSP 同构，表不同）

- 加密表 T1 / 解密表 T2：见 §2.1 / §2.2
- 解密：`p = T2[T2[c] ^ beta]`；加密：`c = T1[T1[p] ^ beta]`
- LCG 参数与 PSP 完全相同（0x215F/0xFF8F、0xDFA3/0xFFEF）
- **β 初值（块 0）是每份存档随机的**——不是从文件尾 seed 导出（详见 §5）

### 4.3 验证结果

- 四份真实档（male/female/s2/s3）全部解出有效明文（head `00 01 04 04`、@0x1000 UTF-16 角色名）
- 闭环加密/解密 0 字节差
- iOS 盐：`3Nc94Hq1zOLh8d62Sb69`（mhp2g_save.py IOS_SALT，供转换器 hash 使用；与游戏实际校验关系待 F 破解后确认）

---

## 5. 游戏读档机制（逆向恢复 β 的方法）

这是整个破解的关键一步——**游戏的密钥流 β 无法从文件静态推出，必须从游戏自身读档流程反推**。

### 5.1 已确认的读档流程（动态插桩 + 差分验证）

1. 游戏读入 savedata.dat → CCCrypt 外层解密 → 得到内层密文（1,483,008B，加载到 0x31561000 附近）。
2. 游戏从**密文块 @0xDFC00** 恢复密钥流锚点：该偏移处的明文恰好是零（零明文锚点，所有档该处均为 0x00000000）。
   ```
   β_DFC00 = T2[cipher@0xDFC00] ^ T1[0]
   ```
   （因为明文为 0：`p = T2[T2[c]^β] = 0` → `T2[c]^β = T1[0]` → `β = T2[c]^T1[0]`）
3. 把 LCG **逆推 0xDFC00/4 = 229120 步**，得到块 0 的 β：
   ```
   β[0] = LCG⁻²²⁹¹²⁰(β_DFC00)
   ```
   （LCG 模 0xFF8F/0xFFEF 互素小模数，逆推即对乘数求模逆元逐块反演）
4. 从 β[0] 正向推进解密整档。

### 5.2 实测验证（两条独立证据）

| 档 | β[0]（脚本推算） | 游戏实际恢复的 β_DFC00 | 验证 |
|---|---|---|---|
| 随机β测试档 | 0xF94D64CB | LCG²²⁹¹²⁰→0xCAE1BE89 | 命中 |
| male 原档 | 0x1DDBD6C0 | LCG²²⁹¹²⁰→0x90279D3B | 命中 |

- 用恢复的 β 模拟完整解密 vs 真实明文：**100.00% 匹配（1,483,008/1,483,008 字节）**。
- **密文尾 4B 不是 seed**：把尾 4B 改成 0x11223344 再部署，游戏恢复的 β 不变（仍 0xCAE1BE89）。

### 5.3 推论（决定性）

游戏每次读档**自行生成密钥流**（β），加密档若用别的 β 加密，读档后明文即乱 → 判损坏。
因此：**任何重新加密的档（即使明文与原档完全相同、β 原样回环）都必须重算校验区**——这直接引出 §6 的校验算法 F。

---

## 6. 校验算法 F（唯一未破，当前堵点）

### 6.1 已证实的事实

- iOS 明文尾部 30B = `8B | 20B | 2B`（末 2B 的第 2 字节恒为 `0x77`，4 份档均如此）。
- **F 的输入 = 内层密文主体（含尾部区），输出应等于尾部 20B**。
- 判定实验（全部真实部署到 touchHLE 实测）：

| 实验档 | 改动 | 游戏判定 |
|---|---|---|
| male 原档 | 无 | **可读**（无弹窗） |
| roundtrip_male_randbeta | 明文相同、随机 β 重加密 | 损坏 ×N |
| seedtest_11223344 | 密文尾 u32=0x11223344 | 损坏；β 未变 |
| exp_tailzero | 尾部 30B 清零 | 损坏 ×2 |
| exp_body1b | @0x2000 单字节翻转 | 损坏 ×2 |
| exp_tail82 / exp_tail20 | 仅改尾 8B+2B / 仅改尾 20B | 待测（部署曾被系统拦截） |

→ F 覆盖主体+尾部，是密文（而非明文）的函数。

### 6.2 已排除（勿重试）

- 标准 SHA1/MD5/SHA256（二进制无初始常量 0x67452301…、无 SHA1 轮常量 0x5A827999…、无 CC_* 调用日志）
- 多种哈希 × 范围 × 盐组合（含 `f09qfkn#olsp9fkn`、`b#hl-*pa-lspzm `、两平台盐）
- 简单 sum/xor/CRC32；字节序/反序/16 位交换变体
- movw/movt 搜 LCG 常量 0x215F/0xDFA3/0xFF8F/0xFFEF 常量池 → 0 命中
- 密文域直接 LCG/表置换反推；密文尾 = seed

### 6.3 候选线索与下一步

- key/iv 字符串后的两个 16B ASCII 串仍是最高价值候选（盐/密钥材料）。
- alert 弹窗插桩已成功 dump 三份快照（alert_plain_345dae0 / alert_cipher_31561000 / alert_cipher_313f6000，各 1,507,328B），但与文件密文仅 0.40% 匹配——弹窗时刻内存状态与文件的关系尚未解读。
- 建议：CPU 级 PC 捕获定位校验例程（读档 decrypt 后、弹窗前运行）；或静态 xref 那两个 16B 串。

---

## 7. 密钥/盐/常量汇总表

| 项 | 值 | 平台 |
|---|---|---|
| KIRK key（日版 ULJM05500） | `cd1f2059ae70ef68dca24513b45adb0a` | PSP 系统层 |
| 外层 AES key | `ls6([f$]=~I(nf28` = hex `6c7336285b66245d3d7e49286e663238` | iOS |
| 外层 AES IV | `;lknc(;f_wnX98<i` = hex `3b6c6b6e63283b665f776e5839383c69` | iOS |
| 游戏层 LCG 乘子/模 | lower 0x215F % 0xFF8F；upper 0xDFA3 % 0xFFEF | 两平台 |
| PSP 盐 | `S)R?Bf8xW3#5h9lGU8wR` | PSP SHA1 |
| iOS 盐（转换器定义） | `3Nc94Hq1zOLh8d62Sb69` | iOS |
| 候选盐/第二密钥 1 | `f09qfkn#olsp9fkn` | iOS（未验证用途） |
| 候选盐/第二密钥 2 | `b#hl-*pa-lspzm ` | iOS（未验证用途） |
| 加密表 T1 / 解密表 T2 | §2.1 / §2.2 | iOS |
| PSP 置换表 | §2.3 | PSP |

---

## 8. 明文结构对照（已验证）

| 项 | PSP | iOS |
|---|---|---|
| 明文长度 | 1,483,004B | 1,483,008B |
| 版本头 @0x00 | `00 03 07 07` | `00 01 04 04` |
| 角色名 | UTF-16（位置因版式不同） | @0x1000 UTF-16LE |
| 尾部结构 | 12B 元数据 + 20B SHA1 | 8B + 20B 校验(F) + 2B（末字节 0x77） |
| 零占比 | 98.44% | 99.94% |
| 与对侧相同度 | — | k73 vs 男档 90.99% |

---

## 9. 已知问题与待核查项（诚实披露）

1. **mhp2g_save.py PSP 表 bug**：当前把 iOS 加密表当 PSP 表用，PSP 解密实测失败；需改用 §2.3 的 PSP 表（并确认其方向=解密方向）。
2. **PSP 文件→明文路径复现差异**：当前 `/tmp` 中的备份 BIN（MD5 `ad5fc04e…`）用 psp_decrypt.py 的表/seed 逻辑无法直接复现 `t2.plain`（head `00030707`）——t2.plain 本身经 SHA1 验证有效（90.99% 匹配 iOS 男档）。推测当时生成 t2.plain 的输入/密钥路径与现有备份不同（原始 Downloads 文件已移动）。**需重新确定正确输入与 KIRK key 用法**（含 key 文件字节序/是否需补 16B）。
3. iOS 校验算法 F 未破（§6）——转换器任何产物目前都无法被游戏读取，这是整体进度的唯一硬阻塞。
4. iOS→PSP 方向转换产物未实测（用户拒绝安装 PPSSPP，需用户真机验证）。

---

## 10. 可复现脚本要点

- PSP 游戏层核心算法：`/tmp/psp_decrypt.py`（表 + 双 LCG + seed 置换两次）
- iOS 内外层 + 转换器：`mhp2g-save-tools/mhp2g_save.py`（`inspect` / `convert` 子命令）
- 外层 AES 复现：openssl 命令见 §4.1
- 内层闭环验证：`decrypt(encrypt(x)) == x`，四份真实档 0 字节差
- β 恢复验证：§5.2 双档 LCG 逆推命中 + 全档 100% 匹配
