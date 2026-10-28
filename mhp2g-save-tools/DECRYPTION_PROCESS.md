# MHP2G 存档解密全流程（PSP ↔ iOS）

> 状态（2026-10-28 修订）：两个平台的整条链路都已破解，并用真实存档逐字节验证。
> - PSP：系统层（sceChnnlsv/KIRK）+ 游戏层 + SHA-1 校验 + `PARAM.SFO` 签名
> - iOS：外层 AES + 游戏层 + SHA-1 校验
>
> 转换器是 Rust crate `mhp2g_save`（命令 `mhp2g-save`，用法见 README.md）。PSP 存档转成 iOS 后，已在 UFtouchHLE 里实际读档成功。
>
> 本文较早的版本有几处错误结论，§9 列出了这些结论及更正。

---

## 0. 文件布局总览

```
PSP 侧（ULJM-05500）：
  MHP2NDG.BIN（0x16A110 = 1,483,024B）+ PARAM.SFO
    │ ① 系统层：sceChnnlsv（KIRK），模式 5，日版游戏 key；16B 文件头 + 数据
    ▼
  游戏层密文 0x16A100B
    │ ② 游戏层解密
    ▼
  明文 0x16A100B

iOS 侧：
  savedata.dat（0x16A110 = 1,483,024B）
    │ ① 外层：CCCrypt AES-128-CBC + PKCS#7（固定 key/iv）
    ▼
  游戏层密文 0x16A100B
    │ ② 游戏层解密（与 PSP 完全相同，同一套表）
    ▼
  明文 0x16A100B
```

两个平台的明文布局相同：

| 偏移 | 内容 |
|---|---|
| 0x000000–0x000024 | 平台设置块（PSP `00 03 07 07 …`，iOS `00 01 04 04 …`） |
| 0x000024–0x16A0DC | 存档数据：3 个角色槽（0x1000 起，每个 0x6B100）+ 全局区。大部分两平台相同，公会卡等例外见 PSP_IOS_FORMAT_COMPARISON.md |
| 0x16A0DC–0x16A0E8 | 12B 元数据，不参与哈希 |
| 0x16A0E8–0x16A0FC | `SHA-1(明文[0:0x16A0DC] + 盐)` |
| 0x16A0FC–0x16A100 | XOR 流的种子（明文形式） |

k73 的 PSP 存档和 iOS 男档明文约 91% 字节相同；不同之处在于平台设置块和角色数据本身。

---

## 1. 通用游戏层算法（两平台完全相同）

```
seed：取密文末尾 4B，逐字节过解密表两次，作为 LE32
lower = seed & 0xFFFF（为 0 则取 0x215F）
upper = seed >> 16  （为 0 则取 0xDFA3）

对除末尾 4B 外的每个 4B 小端字 w（从块 0 开始）：
  w = T2[w 各字节]
  lower = lower * 0x215F % 0xFF8F      ← 先推进，再使用
  upper = upper * 0xDFA3 % 0xFFEF
  w ^= lower | (upper << 16)
  w = T2[w 各字节]
```

- **解密**：`p = T2[T2[c] ^ β]`；**加密**：`c = T1[T1[p] ^ β]`，种子存为 `T1[T1[seed]]`。
- iOS 版的游戏代码：`0x322868`（加解密，参数 1 为读档、0 为写档）、`0x322704`（读档：解密后校验 SHA-1）、`0x322AF4`（写档：算 SHA-1、换种子、加密）。
- 写档时游戏会换一个新种子，因此同一份明文每次存出来的密文都不同。
- 用原种子重新加密，得到的文件与原文件逐字节一致（4 份 iOS 档和 1 份 PSP 档都验证过）。

---

## 2. 置换表

两个平台用**同一套**表。mhef / mhtools 里那张 `A6 21 22 …` 表**不适用于 MHP2G 日版**（见 §9）。

### 2.1 加密表 T1（iOS 二进制文件偏移 0x5F27D0，VA 0x5F67D0；`T1[0]=0x3A`）

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

### 2.2 解密表 T2（文件偏移 0x5F28D0，VA 0x5F68D0；`T2 = T1⁻¹`）

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

---

## 3. PSP 全流程（已验证）

### 3.1 系统层：sceChnnlsv（KIRK）

- `PARAM.SFO` 的 `SAVEDATA_PARAMS[0] = 0x41`，对应加密模式 5；游戏 key（日版 ULJM05500）为 `cd1f2059ae70ef68dca24513b45adb0a`。
- **解密**：
  1. 取文件头 16B，`h = 文件头 ^ 游戏key`。
  2. 求 base 块：`base = AES_dec(KIRK key 0x12, h ^ SD_HASH_KEY7) ^ SD_HASH_KEY6`。
  3. 构造编号块：`c_j = base[0:12] || LE32(j)`，j 从 1 开始。
  4. 求密钥流：`ks_j = AES_dec(KIRK key 0x64, c_j) ^ c_{j-1}`，其中 c_0 = 0。
  5. `数据 ^= 密钥流`。

  加密是同一个运算，只是文件头换成新的随机 16B。
- **签名**（`PARAM.SFO`）：
  - **`SAVEDATA_FILE_LIST` 里的文件哈希**：`AES_enc(KIRK key 0x10, CMAC(KIRK key 0x10, 文件头+密文) ^ SD_HASH_KEY5 ^ 游戏key)`。
  - **`SAVEDATA_PARAMS` 的三个签名**：都是对整个 SFO 算的 AES-CMAC，分别在 +0x20（模式 6）、+0x70（模式 5）、+0x10（模式 1）。模式 6 要经过 KIRK 命令 5，用到写档主机的熔丝 ID，所以只能用编的 ID，和真机对不上。
- 实现：`mhp2g-save-tools/src/psp.rs`。该文件移植自 jpcsp、kirk-engine、psp-savedata-endecrypter（Apollo），因此是 GPL-3.0。

### 3.2 游戏层与校验

- 游戏层：§1，表见 §2。
- 盐：`S)R?Bf8xW3#5h9lGU8wR`；哈希范围是明文 `[0, 0x16A0DC)`。

### 3.3 验证结果（k73 真机存档）

- `MHP2NDG.BIN` 直接解开，角色名「君仔」，SHA-1 吻合。
- 用原文件头重新加密，与原文件逐字节一致，`SAVEDATA_FILE_LIST` 里的文件哈希也与 PSP 写的相同。
- `SAVEDATA_PARAMS` +0x70、+0x10 两个签名（以 PSP 写的 +0x20 为前提）可以复现。
- 与 Apollo 的 `endecrypter` 对照：同样的输入和文件头下，`MHP2NDG.BIN` 逐字节一致；`PARAM.SFO` 只有 `SAVEDATA_PARAMS` 的签名不同，原因是熔丝 ID 不同。

---

## 4. iOS 全流程（已验证）

### 4.1 外层：CCCrypt AES-128-CBC + PKCS#7

| 项 | 值 |
|---|---|
| 算法 | AES-128-CBC，options=1（PKCS7 填充） |
| Key | hex `6c7336285b66245d3d7e49286e663238` = ASCII `ls6([f$]=~I(nf28` |
| IV | hex `3b6c6b6e63283b665f776e5839383c69` = ASCII `;lknc(;f_wnX98<i` |
| 长度 | savedata.dat 0x16A110B → 去掉 16B 填充 → 游戏层密文 0x16A100B |

复现（openssl）：
```
openssl enc -d -aes-128-cbc -K 6c7336285b66245d3d7e49286e663238 \
            -iv 3b6c6b6e63283b665f776e5839383c69 -in savedata.dat -out inner.bin
```

key 和 iv 其实是二进制里两个 32 字节字符串的前半段：`;lknc(;f_wnX98<if09qfkn#olsp9fkn` 和 `ls6([f$]=~I(nf28b#hl-*pa-lspzm s`。后半段（`f09qfkn#…`、`b#hl-*pa-…`）没有用到。

### 4.2 游戏层与校验

- 游戏层：§1，表见 §2。
- 盐：`3Nc94Hq1zOLh8d62Sb69`，与 PSP 美/欧版 MHFU 的盐相同。
- 读档校验（`0x322704`）：先把明文 `0x16A0DC..0x16A0F0` 临时替换成盐，再对前 0x16A0F0 字节做 `CC_SHA1`（二进制里的封装在 `0x2B0B8C`），与 `0x16A0E8` 处的 20B 比较。这等价于 `SHA-1(明文[0:0x16A0DC] + 盐)`。

### 4.3 验证结果

- 4 份真实档（male/female/s2/s3）SHA-1 全部吻合；用原种子重新加密与原文件逐字节一致。
- 在 UFtouchHLE 里实际读档：
  - 原档：可读。
  - 换新种子重新加密：可读。
  - k73 PSP 档转成 iOS：可读，角色选择界面显示「君仔」「无双君仔」。
  - 改 1 字节但不重算 SHA-1：报「游戏存档已损坏」。

---

## 5. 游戏读档机制

1. 读入 savedata.dat，用 CCCrypt 解外层。
2. `0x322868` 解游戏层：种子取自密文末尾 4B，经两次查表还原。
3. `0x322704` 校验 SHA-1，不符就弹「游戏存档已损坏」。

之前重新加密的档被判损坏，原因是旧转换器没把种子写回末尾 4B，游戏解出的明文自然不对。和“校验算法未知”无关。

---

## 6. 校验算法 F（已破解）

F 就是 §4.2 的加盐 SHA-1，和 PSP 是同一套方案，只是盐不同。之前没找到它，有两个原因：
- 只在原始字节里搜 SHA1 常量。游戏用的是 CommonCrypto 的 `CC_SHA1`（导入符号），二进制里本来就没有这些常量。
- 把种子问题误当成了校验问题（见 §5）。

---

## 7. 密钥/盐/常量汇总表

| 项 | 值 | 平台 |
|---|---|---|
| 游戏 key（日版 ULJM05500） | `cd1f2059ae70ef68dca24513b45adb0a` | PSP 系统层 |
| `SAVEDATA_PARAMS[0]` | `0x41`（模式 5 + 模式 1 签名） | PSP |
| 外层 AES key | `ls6([f$]=~I(nf28` = hex `6c7336285b66245d3d7e49286e663238` | iOS |
| 外层 AES IV | `;lknc(;f_wnX98<i` = hex `3b6c6b6e63283b665f776e5839383c69` | iOS |
| 游戏层 LCG 乘子/模 | lower 0x215F % 0xFF8F；upper 0xDFA3 % 0xFFEF | 两平台 |
| 加密表 T1 / 解密表 T2 | §2.1 / §2.2 | 两平台 |
| PSP 盐 | `S)R?Bf8xW3#5h9lGU8wR` | PSP |
| iOS 盐 | `3Nc94Hq1zOLh8d62Sb69`（iOS 二进制 VA 0x51EDBC） | iOS |
| KIRK 密钥、SD 哈希密钥 | 见 `src/psp.rs` | PSP 系统层 |

---

## 8. 明文结构对照

| 项 | PSP | iOS |
|---|---|---|
| 明文长度（含种子） | 0x16A100B | 0x16A100B |
| 设置块 @0x00–0x24 | `00 03 07 07 …` | `00 01 04 04 …` |
| 角色名 | @0x1000 UTF-16LE | @0x1000 UTF-16LE |
| 尾部 | 12B 元数据 + 20B SHA-1 + 4B 种子 | 同左 |
| 零占比（样本） | 98.44% | 99.94% |

---

## 9. 已更正的错误结论

| 早先结论 | 实际情况 |
|---|---|
| iOS 和 PSP 置换表不同，PSP 用 mhef 的 `A6 21 22 …` 表；“用 iOS 表解 PSP 密文失败” | 两个平台用同一套表（§2）。k73 的 `MHP2NDG.BIN` 去掉系统层后，用这套表能解出 SHA-1 吻合的明文，重新加密也与原文件逐字节一致。用 mhef 的表解是乱码。之前的“失败”，应该是没先去掉系统层，或者表的方向用反了 |
| 系统层用日版 key 解出的数据过不了 SHA-1 | 系统层本来就解对了，是游戏层用错了表 |
| 游戏从密文 @0xDFC00 的“零明文锚点”恢复 β，再逆推 229120 步 | 游戏从末尾 4B 读种子（`0x322868`）。“β 锚点”只是同一条 LCG 在中间某处的状态，不是游戏的做法 |
| 密文尾 4B 不是种子 | 是种子，以 `T[T[seed]]` 的形式存放。之前的实验改了末尾 4B，导致整档解出来不对，于是被误读成“β 不变” |
| 校验 F 是密文的未知函数，二进制里没有 SHA1 | F 是对明文加盐的 SHA-1，用的是 `CC_SHA1`（§4.2） |
| iOS 尾部是 `8B + 20B 校验 + 2B（末字节恒为 0x77）` | 那是从密文看的。明文尾部是 12B 元数据 + 20B SHA-1 + 4B 种子 |
| `f09qfkn#olsp9fkn` / `b#hl-*pa-lspzm ` 是候选盐 | 它们是 key/iv 字符串的后半段，没有用到 |

---

## 10. 仍未验证

- iOS → PSP 的转换结果还没在 PSP/PPSSPP 上实际读档。
- 还没在游戏里重新存档，确认转换来的存档能正常回写。
- `SAVEDATA_PARAMS` +0x20 的模式 6 签名用编的熔丝 ID，PSP 和 PPSSPP 是否接受还没实测。
- 平台设置块（0x00–0x24）的字段含义没有逐个确认。

---

## 11. 复现

- 转换器与测试：`mhp2g-save-tools/`
  - 构建：`cargo build -p mhp2g_save --release`
  - 合成数据测试：`cargo test -p mhp2g_save`
- 真实存档核对：`tests/real_saves.rs`，通过环境变量指定存档。
- 外层 AES 复现：openssl 命令见 §4.1。
