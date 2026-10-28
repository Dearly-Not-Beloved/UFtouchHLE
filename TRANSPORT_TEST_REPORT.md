# 独立 P2P 传输层测试报告

日期：2026-10-27。设计及复现命令见 [TRANSPORT_DESIGN.md](TRANSPORT_DESIGN.md)。源码基线为 `47e9bf7c`，本阶段文件尚未提交；保留了前阶段网络追踪和工作区其他改动。

## 测试环境与执行范围

macOS Apple Silicon，本地 release 构建，Rust 标准库 TCP。集成测试绑定 `127.0.0.1:0`，由操作系统分配端口；Host、Client 是分别启动的独立 OS 进程。四人场景同时运行四个相同的探针程序，Host 是其中一名玩家，没有额外服务器。

自动测试入口：[dev-scripts/transport-integration-tests.py](dev-scripts/transport-integration-tests.py)。事件原文保存在 `target/transport-tests/*.jsonl`，子进程 stderr 单独保存；`manifest.json` 记录测试时的平台、UTC 时间、传输源码和实际运行探针的 SHA-256。构建和检查日志位于 `target/network-analysis/transport-*.log`。这些产物均不是发布包，不包含 IPA。

## 已通过的验证

| 场景 | 实际断言 |
| --- | --- |
| 两个独立实例 | Host 和 Client 分别收到对端 PeerConnected，完成真实 TCP 和 v1 握手 |
| 双向二进制消息 | 两个方向逐字节比较空消息、包含 NUL / 255 / 128 的消息、65536 字节最大消息 |
| 有序传输 | 每方向 80 条带序号的消息，分 4 批双向发送，接收内容及顺序严格一致 |
| 消息边界 | 每个 Data 事件对应一条完整输入消息，最大消息跨 worker 的 I/O 预算传输仍完整 |
| API 拒绝路径 | 65537 字节消息同步 TooLarge；Unreliable 同步 UnsupportedDelivery，没有转成 TCP |
| 正常离开 | Client 收到 Stopped(Requested) 并正常退出，Host 观察断开 |
| 进程异常恢复 | 强制杀掉 Client，Host 清除连接；同 ID 的新进程可重新加入并交换数据 |
| 事件背压 | 探针暂停消费 2 秒，Host 连续发送 100 条空消息；Client 明确 Stopped(EventBackpressure)，随后同 ID 新实例可加入 |
| 协议版本 | 人工构造 version=99 的 TCP 头被拒绝，Host 继续工作 |
| 握手期限 | 只发送一个头部字节且保持 socket 开启，仍在握手期限后报 Timeout |
| 心跳期限 | 原始客户端完成 Join 但不回应 Ping，Host 在 peer_timeout 后断开它 |
| 错误序号 | Join 使用 seq=0 后，Data 再用 seq=0，被判 sequence mismatch 并断开 |
| 兼容性 | 正确长度但不同 compatibility tag 返回 Reject 原因码 3 |
| 四人容量 | Host + 三个 Client 的每条直连均收到连接事件，第五名收到 RoomFull |
| 重复身份 | 已在线的 ID 再加入收到 DuplicateIdentity，原有连接保持 |
| Client–Client 直连 | 第三名到第四名、第四名到第二名直接传输并比对载荷 |
| 四人异常恢复 | 杀掉第三名，其余三名观察断开；同 ID 新进程加入后重建所有直连，再次向第四名传数据 |
| Host 丢失 | Host 正常离开后，各 Client 观察 Host 断开并 Stopped(HostLost)，无隐藏房主迁移 |
| 监听冲突 | 同时占用 Host 端口的另一个实例收到异步 AddrInUse 错误 |
| 连接失败 | 本机绑定但未监听的端口触发异步连接失败并 HostLost；macOS 可能报告 ConnectionRefused 或 Timeout，两者均按失败断言 |

纯单元测试新增 5 项，不需要打开 socket：

1. 分片 / 合并输入解析，包含空消息、任意二进制与最大消息。
2. 坏 magic、版本、类型、flags、过大长度、错误 / 重复序号和残帧等待。
3. 名单的数量、长度、ID 唯一性与端口编码验证。
4. 配置中无效目标、队列容量和超时关系的拒绝。
5. 调用侧队列上限、二进制所有权、明确拒绝非可靠模式、过大消息及唯一退出事件。

## 编译与项目回归

| 检查 | 结果与日志 |
| --- | --- |
| `cargo build --release` | 通过；`transport-build.log` |
| `cargo build --release --example transport_peer` | 通过；`transport-example-build.log` |
| `cargo test --release --lib` | 106 通过，1 忽略，0 失败；`transport-unit.log` |
| 多进程传输集成测试 | 全部场景通过；`transport-process.log` |
| 原有 macOS ARM 客体集成测试 | 117 / 117 通过；`transport-guest-integration.log` |
| 前阶段追踪对照测试 | 开关、errno、状态、回调及缺失方法检查通过；`transport-trace-regression.log` |
| `dev-scripts/lint.sh` | 并行调试改动出现前，默认目标 Clippy 与文档生成通过；`transport-lint.log` |
| `cargo clippy --example transport_peer -- --deny warnings` | 通过；`transport-clippy-example.log` |
| 格式与空白 | 传输模块 / 探针 rustfmt 及新增文档 / 脚本检查通过；项目 format.sh 检查通过后出现了并行改动 |

扩展的 `cargo clippy --all-targets -- --deny warnings` **未通过**，余下 8 个诊断均位于未修改的既有代码：`tests/integration.rs` 的 iterator / needless_borrow / useless_format，以及 `ns_string.rs`、`fs/bundle.rs`、`libc/ctype.rs` 的 items_after_test_module。具体输出保留于 `transport-clippy-all.log`；不把这一结果描述为全目标 lint 通过，也不为本任务修整无关测试。

构建仍有既有 Cargo 版本号与 git describe 前缀不匹配、部分 rust-objcopy 找不到 libLLVM 的提示；上表成功构建的退出码为 0。本阶段没有更改工具链。

工作区在本阶段末尾出现其他独立的存档 / 图形 / 输入调试改动；未撤销、提交或归入传输层。表中计数对应实际保存的测试日志，不表示这些并行改动已经在 Android 上验证。

最终再次执行 `cargo build --release --bin touchHLE --example transport_peer` 通过，见 `transport-final-build.log`；该次构建包含并行改动，并额外提示 `src/libc/posix_io.rs:457` 中 `depth` 未使用。未对该无关文件做修复，不能据此宣称最新整个工作区无 lint 警告。随后用这个最终探针重跑多进程集成测试，全部通过；对应二进制及传输源码哈希已保存在上述 manifest 中。

## 结论与限制

本机多进程已验证独立传输层的连接、双向二进制消息、连接内顺序、退出、超时、背压、成员限制及异常后重新加入。Host 只协调成员，不转发其他 Client 的应用载荷。所有测试均未创建模拟器运行时对象或调用客体代码。

**未验证或未实现：**真实双设备 LAN、Android 生命周期 / 防火墙、VPN、广域网、UDP / 丢包低延迟表现、GameKit 兼容、MHP2G 在线入口与双人狩猎。TCP 队头阻塞、设备休眠、长期压力、恶意局域网用户以及动态网络地址变化仍是后续工作。没有认证、加密或断线后的应用消息重放承诺。

本阶段完成后停止，不开始房间发现或游戏对接；没有提交、推送、打 tag 或发布。
