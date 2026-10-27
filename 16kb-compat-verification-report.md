# UFtouchHLE Android 16 KB 内存页兼容性验证报告

- **应用版本**：v0.3.22（package `org.touchhleuf.android`）
- **日期**：2026-10-27
- **目标**：使 ARM64 原生库符合 Android 16 KB 页大小要求，能在 16 KB 页设备上安装与加载

---

## 1. 构建链与版本

| 组件 | 修复前 | 修复后 |
|---|---|---|
| Android NDK | **r25c (25.2.9519653)** | **r28c (28.2.13676358)** |
| Gradle | 8.11.1 | 8.11.1（不变） |
| AGP | 8.10.1 | 8.10.1（不变） |
| cargo-ndk | 3.5.4 | 3.5.4（不变） |
| Rust | 1.99.0 (aarch64-linux-android) | 1.99.0（不变） |
| SDL2（bundled，来自 rust-sdl2 cargo checkout） | 2.26.4 | 2.26.4 + patch |

## 2. 根因分析

Android 系统检测到三个 ARM64 原生库不符合 16 KB ELF LOAD 段对齐：

- `lib/arm64-v8a/libSDL2.so`
- `lib/arm64-v8a/libtouchHLE.so`
- `lib/arm64-v8a/libc++_shared.so`

用 `llvm-readelf -lW` 检查修复前 APK 内三个库，**所有 PT_LOAD 段的 `p_align` 均为 `0x1000`（4 KB）**，而 16 KB 页设备要求 `p_align ≥ 0x4000`。根因是构建使用的 **NDK r25c 默认按 4 KB 对齐 ELF LOAD 段**，且 r25 自带的 `libc++_shared.so` 也是 4 KB 对齐。

## 3. 修复方案

### 3.1 升级 NDK 到 r28c
NDK r28+ 对 64 位目标默认生成 16 KB 对齐的 ELF LOAD 段，且自带的 `libc++_shared.so` 已是 16 KB 对齐。

- `android/app/build.gradle.kts`：`ndkVersion = "28.2.13676358"`
- `.github/workflows/touchHLE_release.yml`：NDK 下载/缓存/环境变量全部由 r25c → r28c

### 3.2 强制 16 KB 链接对齐（Rust 侧）
`libtouchHLE.so` 与 `libSDL2.so` 由 rustc 链接。在 `.cargo/config.toml` 为 `aarch64-linux-android` 目标添加 target 专用 rustflags（仅作用于该目标，不影响 host 构建脚本）：

```toml
[target.aarch64-linux-android]
rustflags = [
    "-C", "link-arg=-Wl,-z,max-page-size=16384",
    "-C", "link-arg=-Wl,-z,common-page-size=16384",
]
```

### 3.3 修复 SDL2 2.26 与 NDK r28 的编译冲突
NDK r28 将 `ALooper_pollAll` 标记为 `__REMOVED_IN(1,…)`（`obsoleted=` 形式，是**硬编译错误**，弱符号宏无法绕过）。`ALooper_pollOnce` 签名相同且可正常使用（上游 SDL 2.28+ 即改用此 API）。

- 新增幂等脚本 `dev-scripts/patch-sdl-ndk28.sh`：定位 cargo 为 rust-sdl2 依赖检出的 SDL 源码（`sdl2-sys/SDL`，即实际被编译的 SDL），把 `ALooper_pollAll` 调用点替换为 `ALooper_pollOnce`
- `android/app/build.gradle.kts`：注册 `patchSdlForNdk28` task，使 `buildCargoNdk*` 在构建前执行该脚本（本地与 CI 一致）

### 3.4 第三方库
- `libc++_shared.so` 由 cargo-ndk 从 NDK 复制，升级到 r28c 后自动为 16 KB 对齐
- `libSDL2.so`、`libtouchHLE.so` 由 3.2 的 rustflags 保证 16 KB 对齐
- 模拟器游戏逻辑未改动（本次未触碰 `src/` 模拟器代码）

## 4. 构建结果

`gradle assembleRelease`（带 `TOUCHHLE_BRANDING=UF`）**构建成功**。

- 产物：`UFtouchHLE_v0.3.22_Android.apk`（约 26 MB）
- package：`org.touchhleuf.android`，versionName `v0.3.22`

## 5. 静态验证（通过）

### 5.1 三个 .so 的 PT_LOAD 对齐（`llvm-readelf -lW`）

| 库 | LOAD 段 `p_align` | 结论 |
|---|---|---|
| libc++_shared.so | `0x4000`（全部 4 段） | ✅ |
| libSDL2.so | `0x4000`（全部 3 段） | ✅ |
| libtouchHLE.so | `0x4000`（全部 4 段） | ✅ |

所有 LOAD 段 offset/vaddr 相对于 16 KB 一致（`offset mod 0x4000 == vaddr mod 0x4000`），布局有效。

### 5.2 APK ZIP 对齐（Google 官方 `zipalign -c -P 16 -v 4`）

```
lib/arm64-v8a/libSDL2.so      (OK - compressed)
lib/arm64-v8a/libc++_shared.so (OK - compressed)
lib/arm64-v8a/libtouchHLE.so  (OK - compressed)
Verification succesful   (exit 0)
```

`zipalign -c -v 4` 常规对齐检查同样通过。

## 6. 实机 / 设备验证

> ⚠️ **本机未能完成模拟器验证**：模拟器启动时 `HVF error: HV_DENIED`，当前环境无法使用 macOS Hypervisor.framework 硬件虚拟化（常见于运行在虚拟化环境或权限受限），纯软件模拟（TCG）不可行。

**实机测试由用户自行进行**。建议在支持 16 KB 页大小的设备（Android 15+ 的 16 KB 设备）上执行：

```bash
# 确认设备为 16 KB 页
adb shell getconf PAGE_SIZE   # 应为 16384
# 安装
adb install -r UFtouchHLE_v0.3.22_Android.apk
# 启动
adb shell am start -n org.touchhleuf.android/.LauncherActivity
```

- **静态对齐检查**：✅ 全部通过（见第 5 节）
- **真实设备运行验证**：⏳ 由用户执行，尚未在本机完成

## 7. 代码修改清单

| 文件 | 修改 |
|---|---|
| `.cargo/config.toml` | 新增 `[target.aarch64-linux-android]` 16 KB rustflags |
| `android/app/build.gradle.kts` | ndkVersion→28.2.13676358；SDL 编译兼容 CFLAGS；patch task hook |
| `.github/workflows/touchHLE_release.yml` | Android 构建 NDK r25c→r28c |
| `dev-scripts/patch-sdl-ndk28.sh`（新增） | 幂等 patch：cargo 检出的 SDL 2.26 `ALooper_pollAll`→`ALooper_pollOnce` |

## 8. 结论

三库 ELF LOAD 段均已 16 KB 对齐，APK 通过官方 `zipalign -P 16` 验证，满足 16 KB 页设备安装与加载的静态要求；模拟器游戏逻辑未改动，保持现有游戏兼容性。实机运行验证待用户在有 16 KB 页的真机上完成。
