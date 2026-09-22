# velm

> 一个用 **Rust** 编写的 Android 原生 **2D GUI 框架**：基于 [vello_gpu](https://github.com/linebender/vello) GPU 渲染 + Elm（TEA）架构 + 零胶水 `NativeActivity`，在 Android 上以 `cdylib` 形式直接运行，无需 Java/Kotlin 层与 Gradle 构建链。

## 仓库构成

| 路径 | 说明 |
|---|---|
| `crates/velm` | 框架核心（rlib）：TEA 运行时、布局、命中测试、vello_gpu 渲染器、NativeActivity 引擎线程 |
| `examples/counter` | 计数器 demo（cdylib），端到端验证载体（SC-2/3/4） |
| `scripts/` | `build_run.sh`（构建/安装/启动/日志）、`stress_lifecycle.sh`（生命周期压测） |
| `docs/` | `SPEC.md`（需求/架构）、`PLAN.md`（里程碑 WBS）、`DECISIONS.md`（ADR）、`prd_v1.md`、`架构设计文档.md`、`spikes/` |

## 架构概览

模块随实现进度开放（详见 `docs/PLAN.md` T1–T16）：

| 模块 | 职责 | 里程碑 |
|---|---|---|
| `app` | `Activity` trait 与运行时状态机 | T8 |
| `view` | `View` / `ViewGroup` / `TextView` 与链式构造器 | T4 |
| `layout` | 手写 LinearLayout 测量与布局（两遍 O(n)） | T6 |
| `event` | `MotionEvent` 与触摸动作映射 | T5 |
| `engine` | NativeActivity 回调、引擎线程、命中测试 | T3/T7/T8/T12 |
| `platform` | `ANativeWindow` 封装与 raw-window-handle | T9 |
| `render` | 绘制指令 + vello_gpu 0.2 / wgpu 30 渲染器 | T2/T10/T16 |
| `error` | 框架统一错误类型（ADR-11） | T10 |

公共 API（`crates/velm/src/lib.rs`）：`Activity`、`Intent`、`MotionEvent`、`TouchAction`、`Color`（peniko 再导出）、`View`、`TextView`、`ViewGroup`、`LayoutParams`、`Background`、`Rect`、`Orientation`、`EdgeInsets`、`LayoutDimension`，以及 android-only 入口 `run_native_activity`。

## 外部渲染依赖（重要）

渲染层依赖本地 path 依赖 `vello_gpu` 0.2 / `vello_common` 0.2 / `glifo` 0.3（wgpu 30）。**这些 crate 来自 vello 仓库的本地工作副本，目录 `vello/` 已加入 `.gitignore`，不纳入本仓库版本控制。**

构建前需将对应版本的 vello 仓库放置于仓库根（与 `vello_gpu` 0.2 / `vello_common` 0.2 / `glifo` 0.3 对应）：

```
velm/
├── Cargo.toml
├── vello/                # 外部渲染仓库工作副本（gitignore，不提交）
│   ├── vello_gpu/        # 0.2.x
│   ├── vello_common/     # 0.2.x
│   └── glifo/            # 0.3.x
├── crates/velm/...
└── examples/counter/...
```

> 若 `vello/` 缺失，`cargo` 会因 path 依赖解析失败而报错。

## 工具链准备

```bash
# Rust 交叉目标（arm64 真机 P0；x86_64 模拟器 P1）
rustup target add aarch64-linux-android
rustup target add x86_64-linux-android

# NDK r27，并导出环境变量
export ANDROID_NDK_ROOT="$HOME/Library/Android/sdk/ndk/27.3.13750724"

# 打包工具 cargo-apk2（无 Gradle，子命令是 `cargo apk2`）
cargo install cargo-apk2

# Android SDK platform-tools（提供 adb）
# macOS: brew install android-platform-tools
```

SDK 默认从 `~/Library/Android/sdk` 探测（亦可设 `ANDROID_SDK_ROOT`）。

## 主机开发检查

纯逻辑模块（view / layout / hit_test / event / 状态机 / app）必须在 host 编译并测试通过：

```bash
cargo build --workspace
cargo test  --workspace                 # host 单测 100（96 velm + 4 counter）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --target aarch64-linux-android -- -D warnings   # android target 零 warning
```

## Android 构建 / 安装 / 运行（counter demo）

打包元数据固化在 `examples/counter/Cargo.toml`（`[package.metadata.android]`：minSdk 24、targetSdk 35、`android.app.NativeActivity`、`lib_name=counter`、MAIN+LAUNCHER）。cargo-apk2 据此**自动生成 `AndroidManifest.xml`**（含 `configChanges`，旋转不重建 Activity，仅走 surface resize）。

```bash
# 一键构建 + 安装 + 启动 + 跟随日志
scripts/build_run.sh                 # release + arm64
scripts/build_run.sh --debug         # debug 构建（vello 日志更全）
scripts/build_run.sh --x86_64        # x86_64 模拟器
scripts/build_run.sh --no-run        # 只构建安装
PKG=rust.counter ACT=android.app.NativeActivity scripts/build_run.sh   # 可用环境变量覆盖

# 等价手动步骤：
cargo apk2 build -p counter --target aarch64-linux-android --release
adb install -r -g target/release/apk/counter.apk
adb shell am start -n rust.counter/android.app.NativeActivity
adb logcat -s VelmEngine AndroidRuntime     # 引擎生命周期/输入/渲染日志
```

- 包名 `rust.counter`、引擎日志 tag `VelmEngine`（cargo-apk2 默认 + spike 实证）。

## 生命周期压测

```bash
scripts/stress_lifecycle.sh              # 默认 20 轮（启停 + Home/回前台 + 旋转）
scripts/stress_lifecycle.sh --cycles 100 # 连启停 100 次
scripts/stress_lifecycle.sh --no-rotate  # 跳过旋转
scripts/stress_lifecycle.sh --tap        # 每轮点一下 +1（顺带压输入路径）
```

脚本每轮统计「同步 ack 已收到」与「引擎线程已 join」次数，并 grep `FATAL / SIGSEGV / abort / ANR / use-after-free`；全量日志落盘 `stress_lifecycle.log`，发现崩溃或 ANR 即非零退出。

**已知行为（v1）**：

- 旋转因 `configChanges` **不重建 Activity**，只走 surface resize → 引擎线程常驻、Model 不丢（SC-5 旋转保状态天然满足）。
- Back / force-stop 销毁 Activity → 引擎线程 Quit+join；再次启动新建 `ActivityRuntime`，Model 重置为 0（v1 不做跨销毁的状态持久化，属预期）。

## 里程碑与验收

| 阶段 | 内容 | 状态 |
|---|---|---|
| T1–T13 | 脚手架 → 渲染 → 输入闭环 | 已完成（host 单测 100，三目标 clippy/fmt 全绿） |
| T14 | 生命周期压测脚本 | 已完成（实跑待设备） |
| T15 | 打包配置与文档 | 已完成（实跑待设备） |
| T16 | 渲染层迁移 vello 0.10 → vello_gpu 0.2（wgpu 30）+ 质量门禁 | 已完成（代码/clippy/test 全绿） |
| Checkpoint C/D | 真机静态画面 + SC-3/4 交互 | 待设备验证 |

完成判据（SPEC §12 的 SC-1~SC-9）需在连机环境逐项取证（命令输出 / 截图 / logcat 归档）。

## 文档索引

- `docs/SPEC.md`：需求与架构规格
- `docs/PLAN.md`：里程碑 WBS（T1–T16）
- `docs/DECISIONS.md`：架构决策记录（ADR）
- `docs/prd_v1.md`、`docs/架构设计文档.md`：产品需求与架构设计
- `docs/spikes/`：关键技术实证

## License

`MIT OR Apache-2.0`（见 `Cargo.toml` `[workspace.package]`）。
