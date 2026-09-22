# velm

一个用 Rust 重写 OpenCode CLI 思路的 **Android 原生 UI 框架**（商业项目，CLI 命令 `forge` 为另一产物）。

本仓库当前包含：

- `crates/velm` —— 框架核心（rlib）：TEA 运行时、布局、命中测试、Vello 渲染器、NativeActivity 引擎线程。
- `examples/counter` —— 计数器 demo（cdylib），作为端到端验证载体（SC-2/3/4）。

> 设计文档见 `docs/`：`SPEC.md`（需求/架构规格）、`PLAN.md`（里程碑 WBS）、`DECISIONS.md`（ADR 决议）、`docs/spikes/`（关键技术实证）。
> 里程碑进度：T1–T13 已完成；T14（生命周期压测）、T15（打包文档）、T16（质量门禁）进行中。

---

## 1. 工具链准备

```bash
# Rust 交叉目标（arm64 真机 P0；x86_64 模拟器 P1）
rustup target add aarch64-linux-android
rustup target add x86_64-linux-android

# NDK r27（ADR-08），并导出环境变量
export ANDROID_NDK_ROOT="$HOME/Library/Android/sdk/ndk/27.3.13750724"

# 打包工具（ADR-04，无 Gradle）
cargo install cargo-apk2        # 子命令是 `cargo apk2`，不是 `cargo apk`

# Android SDK platform-tools（提供 adb）
# macOS: brew install android-platform-tools
```

SDK 默认从 `~/Library/Android/sdk` 探测（亦可设 `ANDROID_SDK_ROOT`）。

---

## 2. 主机开发检查（纯逻辑模块必须在 host 编译通过）

```bash
cargo build --workspace
cargo test  --workspace                 # layout / hit_test / view / event / 状态机
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --target aarch64-linux-android -- -D warnings   # android target 零 warning
```

---

## 3. Android 构建 / 安装 / 运行（counter demo）

打包元数据已固化在 `examples/counter/Cargo.toml`（`[package.metadata.android]`：minSdk 24、targetSdk 35、NativeActivity、`lib_name=counter`、MAIN+LAUNCHER）。cargo-apk2 会据此**自动生成 `AndroidManifest.xml`**（含 `configChanges=0x4a0`，旋转不重建 Activity，仅走 surface resize）。

```bash
# 一键构建 + 安装 + 启动 + 跟随日志（封装在脚本里）
scripts/build_run.sh                 # release + arm64
scripts/build_run.sh --debug         # debug 构建
scripts/build_run.sh --x86_64        # x86_64 模拟器
scripts/build_run.sh --no-run        # 只构建安装

# 等价手动步骤：
cargo apk2 build -p counter --target aarch64-linux-android --release
adb install -r -g target/release/apk/counter.apk
adb shell am start -n rust.counter/android.app.NativeActivity
adb logcat -s VelmEngine AndroidRuntime     # 引擎生命周期/输入/渲染日志
```

> 包名 `rust.counter`、引擎日志 tag `VelmEngine`（均为 cargo-apk2 默认 + spike 实证）。

---

## 4. 生命周期压测（PLAN T14）

```bash
# 先安装（见第 3 节），再跑压测
scripts/stress_lifecycle.sh              # 默认 20 轮（启停 + Home/回前台 + 旋转）
scripts/stress_lifecycle.sh --cycles 100 # 连启停 100 次
scripts/stress_lifecycle.sh --no-rotate  # 跳过旋转
scripts/stress_lifecycle.sh --tap        # 每轮点一下 +1（顺带压输入路径）
```

脚本每轮统计「同步 ack 已收到」与「引擎线程已 join」次数，并 grep `FATAL / SIGSEGV / abort / ANR / use-after-free`；全量日志落盘 `stress_lifecycle.log`，发现崩溃或 ANR 即非零退出。

**已知行为（v1）**：

- 旋转因 `configChanges` **不重建 Activity**，只走 surface resize → 引擎线程常驻、Model 不丢（SC-5 旋转保状态天然满足）。
- Back / force-stop 会销毁 Activity → 引擎线程 Quit+join；再次启动会新建 `ActivityRuntime`，Model 重置为 0（v1 不做跨销毁的状态持久化，属预期）。

---

## 5. 里程碑与验收

| 阶段 | 内容 | 状态 |
|---|---|---|
| T1–T13 | 脚手架 → Provider→ 渲染 → 输入闭环 | 已完成（host 单测 100，三目标 clippy/fmt 全绿） |
| Checkpoint C/D | 真机静态画面 + SC-3/4 交互 | 待设备验证 |
| T14 | 生命周期压测脚本 | 脚本就绪，实跑待设备 |
| T15 | 打包配置与本文档 | 配置就绪，干净 shell 实测待设备 |
| T16 | 质量门禁与文档归位 | 待做 |

完成判据（SPEC §12 的 SC-1~SC-9）需在连机环境逐项取证（命令输出 / 截图 / logcat 归档）。
