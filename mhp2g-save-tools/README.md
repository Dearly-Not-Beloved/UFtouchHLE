# MHP2G 存档转换器（PSP ↔ iOS）

`mhp2g_save.py` 在 PSP 版和 iOS 版《怪物猎人 携带版 2nd G》（MHP2G，iOS bundle id `jp.co.capcom.mhp2gjp`）之间转换存档。只需要 Python 3 标准库，不依赖 pycryptodome 等第三方包。

## 用法

```bash
# 查看存档：平台、角色名、校验和、种子
python3 mhp2g-save-tools/mhp2g_save.py inspect savedata.dat

# PSP → iOS（输入是去掉系统层加密的 PSP 存档，见下文）
python3 mhp2g-save-tools/mhp2g_save.py convert --to ios MHP2NDG.BIN savedata.dat

# iOS → PSP（输出是 PSP 游戏层文件，还需要加上系统层才能给 PSP/PPSSPP 用）
python3 mhp2g-save-tools/mhp2g_save.py convert --to psp savedata.dat MHP2NDG.BIN

# 研究用：导出明文（0x16A100 字节）/ 修改后重新加密（自动重算 SHA-1 和种子）
python3 mhp2g-save-tools/mhp2g_save.py decrypt savedata.dat plain.bin
python3 mhp2g-save-tools/mhp2g_save.py encrypt --platform ios plain.bin savedata.dat
```

- 输入格式自动识别：iOS `savedata.dat`（0x16A110 字节）、PSP 游戏层文件（0x16A100 字节），或明文（0x16A0FC / 0x16A100 字节）。校验和不对的文件会被拒绝。
- `convert` 默认不覆盖已有文件，要覆盖请加 `--force`。写出前会按游戏的方式重新读一遍结果并校验。
- `--template 某个目标平台的存档`：从这个存档取开头的平台设置块（0x00–0x24），不加时用内置的默认值（来自所有已有样本）。
- iOS 存档位置：`touchHLE_sandbox/jp.co.capcom.mhp2gjp/Documents/savedata.dat`。替换前先备份原文件。
- 一次转换要几秒钟（AES 是纯 Python 实现的）。
- 测试：`python3 mhp2g-save-tools/test_mhp2g_save.py`（只用合成数据，不需要游戏文件）。

## 存档格式

两个平台的存档主体都是 0x16A100 字节，布局相同：

| 偏移 | 内容 |
|---|---|
| 0x000000–0x000024 | 平台设置块（PSP `00 03 07 07 …`，iOS `00 01 04 04 …`），转换时替换 |
| 0x000024–0x16A0DC | 存档数据，两个平台相同（0x1000 处是 UTF-16LE 角色名，三个角色槽） |
| 0x16A0DC–0x16A0E8 | 12 字节元数据，不参与哈希 |
| 0x16A0E8–0x16A0FC | `SHA-1(数据[0:0x16A0DC] + 盐)` |
| 0x16A0FC–0x16A100 | XOR 流的种子 |

Capcom 游戏层加密：对除种子外的每个 32 位小端字，`查表 → 推进 LCG → 异或 beta → 查表`。LCG 为 `lo = lo·0x215F mod 0xFF8F`、`hi = hi·0xDFA3 mod 0xFFEF`、`beta = lo | hi<<16`，从种子开始（为 0 的半字换成 0x215F / 0xDFA3）；种子本身经加密表置换两次后存放。

| | PSP（ULJM-05500） | iOS |
|---|---|---|
| 字节表 | 相同：iOS 二进制 VA 0x5F67D0（加密）/ 0x5F68D0（解密）处的表。**不是** mhef / mhtools 里那张 `A6 21 22 …` 表 | 同左 |
| 盐 | `S)R?Bf8xW3#5h9lGU8wR` | `3Nc94Hq1zOLh8d62Sb69`（与 PSP 美/欧版 MHFU 相同） |
| 外层 | PSP 系统存档加密（sceChnnlsv/KIRK AES-128-CBC），日版游戏 key `cd1f2059ae70ef68dca24513b45adb0a`，0x16A110 字节 | AES-128-CBC + PKCS#7（CCCrypt），key `ls6([f$]=~I(nf28`，iv `;lknc(;f_wnX98<i`，0x16A110 字节 |

iOS 版的逻辑来自游戏二进制：`0x322704`（读档：解密 + 校验 SHA-1）、`0x322868`（游戏层加解密）、`0x322AF4`（写档：算 SHA-1、换种子、加密）。

## 验证情况

- iOS：4 份真实存档全部能解密、SHA-1 吻合；用原种子重新加密后与原文件**逐字节一致**。
- PSP：真实的 k73 `MHP2NDG.BIN` 用 Apollo 的 `endecrypter` 加日版 key 解掉系统层后，本工具能读出来，SHA-1 吻合。反方向：本工具写出的 PSP 游戏层文件用 `endecrypter` 加回系统层，再解开，与原文件逐字节一致。
- 在 UFtouchHLE（macOS release 版）里用 P2G 实际读档：
  - 原始存档：正常读取（对照组）
  - 同一存档换新种子重新加密：正常读取（旧版转换器正是这一步失败）
  - k73 的 PSP 存档转成 iOS：正常读取，角色选择界面显示 PSP 存档里的角色「君仔」（15:51，男性）和「无双君仔」（54:31）
  - 改 1 字节但不重算 SHA-1：两次报「游戏存档已损坏」（反向对照，说明检测有效）

## 限制

- **不内置 PSP 系统层加密。** 真机或 PPSSPP 的 `MHP2NDG.BIN` 带系统层，要用外部工具处理。已验证可用的是 Apollo Save Tool 的 `endecrypter`（`key.bin` 是 16 字节的日版游戏 key）：

  ```bash
  # 解密：MHP2NDG.BIN → 游戏层文件，再交给本工具
  endecrypter MHP2NDG.BIN key.bin > game.bin
  # 加密：本工具输出的 PSP 游戏层文件 → MHP2NDG.BIN（会更新同目录存档的 PARAM.SFO，先备份）
  endecrypter game.bin key.bin PARAM.SFO > MHP2NDG.BIN
  ```

  mhef 的 `PSPSavedataCipher` 也能解系统层，但它的 `SavedataCipher` 用的是另一张字节表，读不了日版 MHP2G 的游戏层。
- **iOS → PSP 没有在 PSP/PPSSPP 上实际读档验证过**，只验证了系统层能加回去再解开。
- **只验证了读档。** 没有在游戏里进入任务后再存档，确认转换来的存档能正常回写。
- 平台设置块（0x00–0x24）的含义没有逐字段确认，转换时整体换成目标平台的值；其他字段假定两平台相同（依据：两个平台样本在 0x24–0x1000 完全一致，角色数据结构对得上，游戏能读出角色）。PSP 独有的内容（例如 PSP 下载任务）在 iOS 上的表现未测试。
- 只支持日版 MHP2G（ULJM-05500）和这个 iOS 版。美/欧版 MHFU 的 PSP 存档盐和 iOS 相同，但没有测试。
