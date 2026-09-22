# Implementation Plan: Velm-UI v1.0 最小闭环

- **日期**：2026-09-21
- **依据**：`docs/SPEC.md`（规格）、`docs/DECISIONS.md`（ADR-01~12）
- **计划性质**：SPECIFY 已完成、决策已落；本文件是 PLAN 阶段产物，**review 批准后进入 TASKS/IMPLEMENT**。本计划不含实现代码。
- **v1 完成定义**：SPEC §12 的 SC-1 ~ SC-9（P0）全部达成。

## Overview

按「高风险 spike 先行 → host 纯逻辑 TDD → 平台/渲染 → 引擎闭环 → 生命周期硬化 → 交付」六段推进。前两个 spike 用最小代价消除两个可能推翻架构的不确定点（vello 0.10 在 Android 的真实 API、raw-ndk-sys 符号与零胶水事件模型）；随后 host 可测层（view/layout/event/hit-test/状态机）与设备相关层（window/renderer）两条泳道并行，在静态画面任务汇合，最后接通 TEA 交互闭环并做生命周期压测。

## Architecture Decisions（已在 DECISIONS.md 定稿，摘要）

- **ADR-03**：vello 0.10 正式版 + wgpu 29（放弃 git 0.2 开发快照）；rwh 0.6；peniko 0.6 / skrifa 0.44。
- **ADR-07**：workspace = `crates/velm`（rlib 框架）+ `examples/counter`（cdylib demo），删除 `src/main.rs`。
- **ADR-04**：cargo-apk2 打包，无 Gradle；cargo-ndk 备用。
- **ADR-08**：minSdk 24 / NDK r27 / arm64 P0、x86_64 P1。
- **ADR-09 + SPEC §3.3**：回调只发布事件；独立引擎线程持有 Looper 并 attach 输入队列；窗口/队列销毁走同步协议；按需重绘。
- **ADR-01**：移除 ort/rstar/redb/jni/image/rayon/serde*；保留 crossbeam-channel/pollster/bytemuck/log。

## 依赖图

```text
T1 workspace 骨架 + 空 cdylib 装机
   ├──► T2 渲染 spike (vello0.10/wgpu29/rwh0.6/文本 POC) ─┐
   └──► T3 NDK spike (符号核对/引擎线程 Looper/channel) ─┤
                    │                                      │
   CP-A（spike 评审，定稿 ADR-02/06 细节）                 │
                    │                                      │
   ┌────────────────┴───────────────┐                      │
   │ 逻辑泳道（host，TDD）           │                      │
   │ T4 view ──► T6 layout          │                      │
   │   │      └► T7 hit-test        │                      │
   │ T5 event（与 T4 并行）          │                      │
   │ T6/T7/T5 ─► T8 app契约+状态机  │                      │
   └────────────────┬───────────────┘                      │
                    │                                      │
   平台泳道：T3 ─► T9 window(rwh0.6) ─► T10 renderer ◄──────┘
                    │                                      │
                    └──────────────┬───────────────────────┘
                                   ▼
                      T11 静态画面接线 ─► CP-C
                                   ▼
                      T12 引擎线程/回调重写 ─► T13 输入交互闭环 ─► CP-D
                                   ▼
                      T14 生命周期硬化 ─► T15 打包/文档 ─► T16 质量门禁 ─► CP-E
```

## 并行机会

- **T2 / T3**：渲染与 NDK 两个独立子系统，spike 可并行（不同会话/agent）。
- **逻辑泳道 T4~T8 与平台泳道 T9~T10**：前者只依赖 peniko（host 可编译），后者只依赖 spike 结论；两泳道全程并行，T11 汇合。
- T6 与 T7 在 T4 完成后并行；T5 与 T4 并行。
- **必须串行**：workspace 骨架（T1）→ 一切；T11 → T12 → T13（同一引擎文件演进）；T14 在闭环之后。

---

# Task List

## Phase 0 — Spike 与骨架（高风险先行）

### Task 1: workspace 骨架 + 空 NativeActivity cdylib 装机

**Description:** 按 ADR-07 把根 `Cargo.toml` 转为 workspace；新建 `crates/velm`（rlib）与 `examples/counter`（cdylib）；安装 cargo-apk2（ADR-04），配置 NDK r27（ADR-08）与 minSdk 24；demo 导出 `ANativeActivity_onCreate`，初始化 android_logger 并打印生命周期日志、绑定 5 个回调（回调体仅日志），打 APK 装到 arm64 真机/模拟器启动。删除 `src/main.rs`。

**Acceptance criteria:**
- [x] `cargo build -p counter --target aarch64-linux-android` 产出的 `libcounter.so` 中 `nm -D` 可见 `ANativeActivity_onCreate`（arm64 静态验证；DT_NEEDED 仅 liblog/libdl/libc）
- [x] cargo-apk2 打包安装后启动，logcat 可见 onCreate 与回调绑定日志（tag `VelmEngine`）——**x86_64 / API 36 模拟器动态验证通过**（2026-09-21）；arm64 真机待连机复验
- [x] workspace 两 crate 均 `cargo build` 通过；`src/main.rs` 已删除；`Cargo.toml` 不含 ADR-01 移除项

**Verification:**
- [x] `cargo metadata --format-version 1` 无错误（`cargo apk2 check` 通过）
- [x] `adb logcat -s VelmEngine` 抓到启动日志；App 不闪退（另实测 Home/回前台 surface 销毁重建日志正常）
- [x] `cargo clippy --workspace --target aarch64-linux-android -- -D warnings`（host + arm64 + x86_64 三目标 clippy/fmt 全绿）

> **实施记录（2026-09-21，T1 完成）**：cargo-apk2 1.4.1 子命令为 `cargo apk2 check/build/run`，需 `ANDROID_NDK_ROOT`；metadata 格式、默认 `configChanges=0x4a0`（旋转不重建 Activity）、raw-ndk-sys 高版本 stub 链接坑（由 `examples/counter/build.rs` 补 API29 `-L` 解决）详见 `docs/spikes/2026-09-ndk-capabilities.md`。计划外新增文件：`.cargo/config.toml`、`examples/counter/build.rs`。关键实证：未 `attachLooper` 时按键 5s ANR（预期，T3/T12 闭环）。

**Dependencies:** None
**Files likely touched:** `Cargo.toml`（根 workspace）、`crates/velm/Cargo.toml`、`crates/velm/src/lib.rs`、`examples/counter/Cargo.toml`、`examples/counter/src/lib.rs`（cargo-apk2 manifest 元数据置于 Cargo.toml，不额外建文件）
**Estimated scope:** M（5 文件）

### Task 2: 渲染 spike —— vello 0.10 / wgpu 29 / rwh 0.6 / 文本 POC

**Description:** 在 counter（或临时 spike crate）中验证完整渲染路径：从 `ANativeWindow` 裸指针构造 rwh 0.6 句柄 → wgpu Instance/Adapter/Device/Surface（wgpu 29，SurfaceTargetUnsafe::RawHandle）→ 配置 Rgba8Unorm surface → vello 0.10 初始化与 Scene 编码（清屏 #121212、一个圆角矩形、一行英文、一行中文）→ present（pollster）。按 ADR-06 时间盒对比 glifo 0.2 与 skrifa 直绘，选定文本方案并记录系统字体加载方式（Roboto / NotoSansCJK ttc）。

**Acceptance criteria:**
- [x] 真机窗口出现深色背景 + 彩色圆角矩形 + 清晰的中英文文本各一行（Slice 2/3，证据见 render-poc §3.5/§4.5）
- [x] 窗口尺寸变化（旋转）后 surface 重配不崩、画面正确（Slice 4，竖↔横铺满）
- [x] spike 笔记明确：vello 0.10 真实 API 调用序列、wgpu 特性/呈现模式选择、文本方案（**定稿 skrifa 直绘**，ADR-06）、字体加载与 ttc 处理、遇到的版本坑（render-poc §2~§5）

**Verification:**
- [x] 设备截图存档（暗底/矩形/中英文可见）
- [x] 旋转 3 次无崩溃、无 surface 错误日志（2026-09-22 实测竖→横→竖→横 3 次方向切换，含启动重复上报共 4 次 Resized，0 surface 错误 / 0 崩溃）
- [x] 产出 `docs/spikes/2026-09-render-poc.md`，结论可直接指导 T10（§2.6/§3.6/§4.6/§5.5 约束清单）

**Dependencies:** T1
**Files likely touched:** `crates/velm/src/render/{mod.rs,text.rs}`、`crates/velm/src/engine/activity_thread.rs`（resize 接线）、`docs/spikes/2026-09-render-poc.md`（POC 落在框架内，T10/T12 演进）
**Estimated scope:** M（设备验证为主；本任务以消除不确定性为目标，不追求结构）

> **实施记录（2026-09-22，T2 完成，4 切片 4 commit：3830e93 / d6e1415 / 290ae64 / Slice4）**：POC 落在框架 `crates/velm/src/render/`（非 counter）。关键实证：① 模拟器 SwiftShader Vulkan 可用，但 ranchu `vkSetDebugUtilsObjectNameEXT` 缺陷致 request_device 段错误 → `InstanceFlags::DISCARD_HAL_LABELS`；② vello0.10 无 render_to_surface，走「Rgba8Unorm 中间纹理(STORAGE|TEXTURE) + `wgpu::util::TextureBlitter`」；③ **色彩管线**：vello fine pass 输出字节已是 sRGB 编码结果，surface 必须强制**非 sRGB `Rgba8Unorm`**（blitter 同格式），否则双重编码泛白；limits 用 `Limits::default()`；④ 文本定稿 skrifa0.44 直绘（ADR-06），系统字体 Roboto(index0)+NotoSansCJK ttc **index2=SC**，逐字符双字体回退；⑤ resize 走 `onNativeWindowResized`（ndk-build2 默认 configChanges 已拦截旋转重建），重配 surface + 重建中间纹理 + 补帧。日志级别固定 Info（Trace 冲爆 logcat ring buffer 丢早期日志）。**未覆盖**：arm64 真机动态（仅 x86_64 模拟器动态 + aarch64 静态/clippy 绿）、Outdated/Lost 运行时真实触发。

### Task 3: NDK 能力 spike —— 符号核对、引擎线程 Looper、通道模型

**Description:** 按 ADR-02 核对 raw-ndk-sys 0.1.2 是否提供：`ALooper_prepare/wake/pollOnce`、`AInputQueue_attachLooper/detachLooper/getEvent/preDispatch/finish`、`ANativeWindow_acquire/release/getWidth/getHeight/setBuffersGeometry`、`AConfiguration_fromAssetManager/getDensity`、`AMotionEvent_*`；缺失项记录并评估 ndk-sys 0.6 兜底。POC 验证：主线程回调只通过 crossbeam-channel 发布事件，spawn 的引擎线程 `ALooper_prepare(ALLOW_NON_CALLBACKS)` 后把输入队列 attach 到自己的 Looper 并能被唤醒收到事件；验证窗口/队列销毁的同步 ack 时序。

**Acceptance criteria:**
- [x] 输出符号核对清单（每个需要的符号：存在/缺失/签名差异），给出最终绑定选择（raw-ndk-sys 或 ndk-sys）——**全部存在，raw-ndk-sys 定稿，见 spike §4**
- [x] POC 真机证明：回调线程不阻塞；引擎线程收到 WindowCreated/QueueCreated/触摸事件；销毁事件能被同步送达并干净退出
- [x] 确认 density 获取路径与返回值（真机 dpi）——x86_64/API36 dpi=160 → density=1.00

**Verification:**
- [x] logcat 时序日志显示「回调立即返回、引擎线程消费」（主线程 pid 与引擎线程 `velm-engine` 两个线程号分明）
- [x] 反复启停 10 次无卡死/崩溃（2026-09-22 实测 10/10 干净 join、0 ANR、0 FATAL；另验 Home/回前台 surface 重建 + 输入恢复）
- [x] 产出 `docs/spikes/2026-09-ndk-capabilities.md`

**Dependencies:** T1
**Files likely touched:** `crates/velm/src/engine/activity_thread.rs`（POC 落在框架内，T12 演进）、`docs/spikes/2026-09-ndk-capabilities.md`
**Estimated scope:** M

> **实施记录（2026-09-22，T3 完成）**：POC 直接落在框架 `activity_thread.rs`（而非 counter），引擎线程模型即 T12 骨架。提交：f52902c（符号）、0d5f8e0（Looper/输入闭环）、本次（窗口所有权/density/压测）。计划外实证：① **不提交首帧则 InputWindowHandle frame=0×0、触摸不投递**（T10 硬约束，spike §5.1）；② bindgen 事件类型常量为 u32、getType 为 i32；③ Home 只销毁 window 不销毁 input queue（回前台仅重建 surface，queue 一直 attached）；④ `post_probe_frame` 为临时软件帧，T10 删除。

### Checkpoint A — Spike 评审（与人 review 后才能继续）

- [x] T2/T3 真机证据齐全；ADR-02（绑定）、ADR-06（文本栈）从「spike 决定」变为「实测定稿」（ADR-03 渲染栈亦于 T2 四切片后定稿）
- [x] vello 0.10 + wgpu 29 路径可行；若不可行，回到 DECISIONS 重新决策（不允许带疑问进入正式开发）——**已真机走通清屏/Scene/文本/resize，无遗留阻断性疑问**
- [x] 零胶水事件模型时序得到真机验证；SPEC §3.3 无需推翻（T3 10/10、T2 resize 同步时序）

> **状态（2026-09-22）**：以上事实项由 AI 自检齐备，T2/T3 两个高风险 spike 全部闭环。**CP-A 仍待人类 reviewer 正式评审放行**；在获得批准前不进入 Phase 1（T4+）正式开发。

## Phase 1 — host 纯逻辑层（逻辑泳道，TDD）

### Task 4: view 模块（params / text_view / view_group）

**Description:** 按 SPEC §7.2 实现 `LayoutDimension`、`Orientation`、`EdgeInsets`、`LayoutParams`（Default = WrapContent/0 margin）、`Rect::contains`、`Background`（color + corner_radius，ADR-10）、`TextView<Msg>`、`ViewGroup<Msg>`、`View<Msg>` 枚举与链式构造器（`linear_layout`、`text_view`、`set_text_size/color`、`set_background`、`set_on_click_listener`）；对容器设字号为 no-op + trace。

**Acceptance criteria:**
- [x] host `cargo build -p velm` 通过（无 android 依赖）
- [x] 默认值、链式设置（含背景色/圆角）、点击消息绑定、no-op 行为均有单测断言
**Verification:** `cargo test -p velm view`、`cargo clippy -p velm -- -D warnings`
**Dependencies:** T1（仅需 workspace 存在）
**Files:** `crates/velm/src/view/{mod.rs,params.rs,text_view.rs,view_group.rs}`、`crates/velm/tests/view.rs`
**Estimated scope:** M（5 文件）

> **实施记录（2026-09-22，T4 完成）**：`peniko 0.6` 由 android-only 依赖提升为通用 `[dependencies]`（SPEC §10.2 要求 view/layout/hit_test 在 host 可编译，peniko 为 `#![no_std]` 纯 Rust，android 侧不受影响）。新增文件 `crates/velm/src/view/{mod,params,text_view,view_group}.rs` + `crates/velm/tests/view.rs`，`lib.rs` 无条件 `pub mod view`。**13 个 host 单测全绿**（默认值、`Rect` 闭区间四边界/四角、链式背景+圆角、点击消息绑定、容器 `set_text_size` no-op、`set_layout_params`、counter demo 嵌套建树）。门禁：host + aarch64 + x86_64 三目标 `cargo build/clippy -D warnings` 全绿，`cargo fmt --check` 干净。
>
> **两处规格补充（已回写 SPEC §7.2）**：① 新增 `.set_layout_params(LayoutParams)` —— 规格原构造器清单没有覆盖宽高的链式入口，而 T6/T11 的按钮必须指定 `Dp`；② no-op 误用提示**只记 `log::trace!`、不启用 `debug_assert`** —— debug 构建下 panic 会使 no-op 路径不可测，与 docs 静默语义冲突。
>
> **API 形状实证**：`peniko 0.6` 无根级 `Color` 定义，实为 `pub type Color = color::AlphaColor<color::Srgb>`；`AlphaColor` **只派生 `Clone/Copy/Debug`、无 `PartialEq`**，故 `Background` 不派生 `PartialEq`，测试改用 `to_rgba8()` 比对（`Rgba8` 有 `PartialEq`）——T10 渲染层沿用此比对方式。

### Task 5: event 模块（action 纯解码 + FFI 壳）

**Description:** 按 SPEC §7.3 实现 `TouchAction`、`MotionEvent` 与纯函数 `decode_action(raw: u32) -> Option<TouchAction>`（`&0xff` 掩码 + DOWN/MOVE/UP/CANCEL/非法值）；`unsafe from_ndk` 仅在 `cfg(target_os="android")` 编译，负责类型判断与 getX/getY(index=0) 后调用纯函数。常量类型以 T3 清单为准，不保留无依据强转。

**Acceptance criteria:**
- [x] host 单测覆盖 4 种 action、带 pointer index 位的掩码值、非法值返回 None
- [x] android 构建通过且 FFI 壳内无业务分支
**Verification:** `cargo test -p velm event`；`cargo clippy -p velm --target aarch64-linux-android -- -D warnings`
**Dependencies:** T3（符号清单；纯函数部分可先做）
**Files:** `crates/velm/src/event/{mod.rs,motion_event.rs}`、`crates/velm/tests/event.rs`
**Estimated scope:** S（3 文件）

> **实施记录（2026-09-22，T5 完成）**：新增 `crates/velm/src/event/{mod,motion_event}.rs` + `crates/velm/tests/event.rs`，`lib.rs` 无条件 `pub mod event`（顺带修正模块清单中 T5/T6 编号错位）。**8 个 host 单测全绿**（4 种 action、带 pointer index 的高 8 位掩码、掩码幂等、POINTER_DOWN/UP 与 HOVER/SCROLL/BUTTON 等不支持 action、`u32::MAX` 等非法值、getter、Clone）。
>
> **关键实证（T3 清单已验证落地）**：`AMOTION_EVENT_ACTION_*` 常量在 raw-ndk-sys 中类型为 `u32`（`_bindgen_ty_23 = c_uint`），`AInputEvent_getType` / `AMotionEvent_getAction` 返回 `i32`（需 `as i32` / `as u32` 显式转换，spec 要求不留无依据强转）。模块内手写 u32 常量（host 不可用 raw-ndk-sys），并用 `#[cfg(target_os = "android")] const _: () = { assert!(...) }` 在 **android 编译期**校验其与 NDK 绑定一致，防止漂移。
>
> **规格澄清（已回写 SPEC §7.3）**：`decode_action` 在函数**内部**做 `& 0xff`，对已掩码 / 未掩码输入幂等；v1 单点，多点触控 action 返回 `None`。
>
> 门禁：host + aarch64 + x86_64 三目标 clippy `-D warnings` 全绿，`cargo fmt --check` 干净；累计 21 个 host 单测（T4 13 + T5 8）。**未覆盖**：`from_ndk` 只能在设备侧验证，留待 T13 真机闭环。

### Task 6: layout 测量（margin / density / WrapContent 修正）

**Description:** 按 SPEC §7.4 + ADR-12 实现 `measure_and_layout(root, w_px, h_px, density)`：Dp/sp 乘 density；margin 四向生效；TextView WrapContent 用文本度量（spike 文本方案提供 advance；未接入前用规格近似并隔离为可替换函数）；ViewGroup WrapContent 按子节点与 margin 求和（修正 docs 缺陷）；输出绝对像素坐标。

**Acceptance criteria:**
- [x] 单测覆盖：MatchParent/Dp/WrapContent、纵/横排列、margin 偏移、density=2 时 Dp 翻倍、根容器铺满、clamp 不溢出
**Verification:** `cargo test -p velm layout`
**Dependencies:** T4（T2 文本度量结论可后补，先用可替换 trait/函数隔离）
**Files:** `crates/velm/src/layout/{mod.rs,measure.rs}`、`crates/velm/tests/layout.rs`
**Estimated scope:** S（3 文件）

> **实施记录（2026-09-22，T6 完成）**：新增 `crates/velm/src/layout/{mod,measure}.rs` + `crates/velm/tests/layout.rs`；`lib.rs` 无条件 `pub mod layout`。**16 个 host 单测全绿**（累计 37 = T4 13 + T5 8 + T6 16）。docs 的**两个缺陷均已修正并有回归单测**：① 容器 WrapContent 由子节点「主轴求和 / 交叉轴取最大」得到（回归用例 `view_group_wrap_content_is_not_match_parent`）；② margin 四向生效，既偏移位置也扣减可用尺寸（回归用例 `margin_reduces_match_parent_size` / `sibling_margins_separate_children`）。
>
> **算法**：两遍 O(n)——`measure` 求尺寸（容器 WrapContent 依赖子节点，必须先于定位）、`place` 写绝对坐标；每遍各遍历一次全树，不做重复递归。**文本度量隔离为 `pub fn estimate_text_width(text, text_size_px)`**，P1 换 skrifa 真实 advance 时只改这一个函数。
>
> **两处规格补充（已回写 SPEC §7.4）**：① 根节点的可用区 = 整窗尺寸扣除根自身 margin（否则 MatchParent 根节点带 margin 会溢出屏幕）；② 明确 v1 限制：WrapContent 容器内的 MatchParent 子节点按上界一次求值，不实现 Android 的二次 measure。
>
> **后续修正（2026-09-22，T7 发现）**：margin 未乘 density（三处消费点把 dp 当像素用），已修并补两条 density≠1 回归，详见 SPEC §7.4 第 6 条与 T7 实施记录。
>
> 门禁：host + aarch64 + x86_64 三目标 clippy `-D warnings` 全绿，`cargo fmt --check` 干净。

### Task 7: hit_test DFS

**Description:** 按 SPEC §7.5 实现 `perform_hit_test<Msg: Clone>`：容器 rect 不包含直接排除、子节点逆序探测、容器回落、闭区间边界。
**Acceptance criteria:**
- [x] 单测覆盖：叶子命中/未命中、重叠时后添加者优先、容器自身监听回落、边界点坐标
**Verification:** `cargo test -p velm hit_test`
**Dependencies:** T4
**Files:** `crates/velm/src/engine/{mod.rs,hit_test.rs}`、`crates/velm/tests/hit_test.rs`
**Estimated scope:** S（3 文件）

> **实施记录（2026-09-22，T7 完成）**：新增 `crates/velm/src/engine/hit_test.rs` + `crates/velm/tests/hit_test.rs`；`lib.rs` 改为无条件 `pub mod engine`，`engine/mod.rs` 内只对 `activity_thread` 子模块保留 `#[cfg(target_os = "android")]`——否则命中测试失去 host 可测性（§10.2 硬约束）。**15 个 host 单测全绿**（累计 54 = T4 13 + T5 8 + T6 18 + T7 15）。
>
> **语义澄清（已回写 SPEC §7.5）**：容器「回落」发生在子节点 `find_map` 之内——子节点及其后代都不产生消息时继续探测更下层兄弟节点，因此**未绑监听的容器对点击是透明的**，只有绑了监听的容器才会拦下点击。两条回归用例分别钉住穿透与拦截。
>
> **顺带修掉一个 T6 缺陷**：端到端用例 `hit_after_real_layout_matches_dp_geometry` 发现 margin 未乘 density——`place` / `measure` / `main_extent` 三处把 dp 值直接当像素用，density=1 时结果一致故 T6 单测未暴露；density=2 时外边距只有应有值的一半。已统一收敛到 `margin_px(margin, density)` 换算，并补 `margin_is_scaled_by_density` / `sibling_margins_are_scaled_by_density` 两条 density≠1 回归（SPEC §7.4 第 6 条）。
>
> 门禁：host + aarch64 + x86_64 三目标 clippy `-D warnings` 全绿，`cargo fmt --check` 干净。

### Task 8: app 契约 + 引擎事件状态机（纯逻辑）

**Description:** 按 SPEC §7.6/§3.4 实现 `Activity` trait、`Intent::none()` 占位；定义引擎输入事件枚举（WindowCreated/WindowDestroyed/QueueCreated/QueueDestroyed/Touch(MotionEvent)/Quit）与不依赖 Looper 的状态机 `step(state, event) -> Vec<Action>`（动作：创建/销毁 surface、绑定队列、入队消息、重绘、退出），把「销毁到达顺序任意」「无窗口丢弃重绘」等不变量做成 host 单测。
**Acceptance criteria:**
- [x] 单测覆盖 SPEC §3.4 全路径：窗口先于/晚于队列、销毁后重建、无窗口时消息丢弃策略、Quit 后不再产生动作
**Verification:** `cargo test -p velm`（全套）
**Dependencies:** T5、T6、T7
**Files:** `crates/velm/src/app/{mod.rs,activity.rs,state.rs}`、`crates/velm/src/engine/events.rs`、`crates/velm/tests/state_machine.rs`
**Estimated scope:** M（5 文件）

> **实施记录（2026-09-22，T8 完成）**：新增 `app/{mod,activity,state}.rs`、`engine/events.rs`、`tests/{state_machine,app}.rs`；`lib.rs` 加 `pub mod app`。**27 个新 host 单测全绿**（累计 81 = T4 13 + T5 8 + T6 18 + T7 15 + T8 27，其中 state_machine 17 + app 10）。
>
> **文件归属的两处调整（与上方 Files 清单略有出入，理由如下）**：① 生命周期状态机放在 `engine/events.rs` 而非 `app/state.rs`——它管的是窗口 / 队列生命周期，属引擎层，放在 `app` 会让 T12 写出 `use crate::app::state::EngineState` 这种逆分层引用；`app/state.rs` 改为承载 `ActivityRuntime`（TEA 运行时：Model + 消息队列 + 重绘标志）。② 增补 `tests/app.rs` 承载 Activity 契约与运行时用例，`tests/state_machine.rs` 只管生命周期。
>
> **定稿语义（已回写 SPEC §3.4 / §7.6）**：重绘用**标志位** `take_draw_request()` 而非 `EngineAction::Redraw` 表达；窗口销毁清除未消费的重绘标志；`WindowResized` 沿用旧 density（回调不携带）；`Quit` 先 `DestroySurface` → `DetachQueue` 再 `Exit`，置位后任何事件返回空动作集；重复创建先销毁旧资源、重复销毁忽略，均 warn 不 panic。`Intent` 的 Clone/Copy/Default/Debug 手写实现，避免 derive 给 `Message` 加约束。
>
> 门禁：host + aarch64 + x86_64 三目标 clippy `-D warnings` 全绿，`cargo fmt --check` 干净；android 双 target `cargo build --workspace` 通过。**未覆盖**：`activity_thread` 与状态机的接线（T12）、真机生命周期验证（T13）。

### Checkpoint B — 纯逻辑层完成

- [x] host `cargo test --workspace` 全绿（88 个 host 单测）；layout/hit/event/状态机覆盖 ≥85% 行 —— **实测 99.33% 行 / 100% 函数，见下方状态**
- [x] `cargo fmt --check`、host clippy 零告警
- [x] 逻辑泳道代码 grep 不到任何 android FFI 符号

> **状态（2026-09-22，T8 完成后自检）**：`cargo test --workspace` 全绿（81 = T4 13 + T5 8 + T6 18 + T7 15 + T8 27）；`cargo fmt --check` 与 host `clippy -D warnings` 干净；对 `view/`、`layout/`、`app/`、`event/`、`engine/{events,hit_test}.rs` 全量 grep `raw_ndk|ANativeWindow|AInputQueue|ALooper|AInputEvent|extern "C"` **零命中**，android FFI 只存在于 `engine/activity_thread.rs`。覆盖率门槛**已实测达标（2026-09-22）**：`cargo llvm-cov --workspace --ignore-filename-regex '(activity_thread|window)\.rs'`（排除只能在设备侧运行的 FFI 文件）→ **行覆盖 99.33%（450 行 / 未覆盖 3）、函数覆盖 100%（58/58）、区域覆盖 99.09%**。分项：`view/*`、`app/state.rs`、`engine/hit_test.rs`、`event/motion_event.rs`、`platform/mod.rs` 均 100%；`layout/measure.rs` 99.34%（1 行）、`engine/events.rs` 97.65%（2 行）、`app/activity.rs` 100%（补 `Intent::clone` 与 trait 默认 `on_touch_event` 两处用例后从 64.7% 拉满）。工具链：`cargo-llvm-cov 0.9.1` + `rustup component add llvm-tools-preview`（本机未预装，约 26 分钟安装）。

## Phase 2 — 平台与渲染（平台泳道，可与 Phase 1 并行）

### Task 9: platform/window（rwh 0.6 + 所有权 + density）

**Description:** 按 SPEC §7.1、ADR-11/12 实现 `NativeWindowWrapper`：`from_ndk`（acquire）、`Drop`（release）、`configure_buffers`（RGBA_8888，失败返回错误码不 panic）、size/format、rwh 0.6 `HasWindowHandle/HasDisplayHandle`；density 经 AConfiguration 获取并封装为 `ScreenConfig { density, w, h }`。
**Acceptance criteria:**
- [x] android 构建通过（`clippy -D warnings` + `cargo build --workspace` 双 target）；真机日志打印正确宽高/density；重复 acquire/release 平衡（日志/压测无窗口泄漏报错）——**真机两项待 T13**
- [x] rwh 0.6 句柄能被 T10 的 wgpu 接受（T10 接线闭合：`VelloRenderer::new` 用 `window.window_handle()/display_handle()` 的 raw 句柄调 `create_surface_unsafe`）
**Verification:** `cargo clippy -p velm --target aarch64-linux-android -- -D warnings`；真机 logcat
**Dependencies:** T3
**Files:** `crates/velm/src/platform/{mod.rs,window.rs}`（density 若超过 2 文件上限放 window.rs 内）
**Estimated scope:** S

> **实施记录（2026-09-22，T9 完成）**：新增 `platform/{mod,window}.rs` + `tests/platform.rs`；`lib.rs` 加 `pub mod platform`。**5 个 host 单测全绿**（累计 86 = 前 81 + T9 5）；host/aarch64/x86_64 三目标 clippy `-D warnings` 与 android 双 target build 全绿。
>
> **host 可测性**：`ScreenConfig` 与 `density_from_dpi` 放 `platform/mod.rs`（host 可见），`window` 子模块 android-only gate——同 `engine/hit_test` 的 gate 约定。density 常量（160 / ANY 65534 / NONE 65535）手写并用 android 编译期 `const assert` 校验防漂移。
>
> **所有权语义（已回写 SPEC §7.1）**：`from_ndk` 自 acquire、`Drop` 自 release，`owned` 标志记录状态；**T12 重写回调时必须移除 T3 spike 在 `native_window_created` 里的那次 acquire**，否则引用计数不平衡、窗口永不释放。包装类型不实现 `Send`/`Sync`，窗口只在引擎线程构造与释放。
>
> **待验证**：`from_ndk`/`from_ndk_owned`/`configure_buffers` 的实际行为只能在设备侧验证（T13）。rwh 句柄已在 T10 接入 wgpu，编译期契约成立、运行时行为待真机。

### Task 10: render/vello_renderer

**Description:** 按 SPEC §7.8 与 T2 结论实现 `VelloRenderer::new(window,w,h)?`、`resize`、`render(root: &View<Msg>)`：wgpu 29 surface/device（Fifo 呈现、surface 能力自适应格式）、vello 0.10 编码（#121212 清屏、`Background` 圆角矩形、TextView 文本按 text_size/color/computed_rect、中英文字体 fallback）、present 与 OUTDATED/LOST 恢复；`velm::Error`（thiserror）落地。
**Acceptance criteria:**
- [x] 绘制指令生成有 host 单测：顺序（画家算法）、sp→px、零面积/空文本跳过、与 `measure_and_layout` 串联的坐标一致、基线居中
- [ ] 给定一棵硬编码已布局 View 树，真机渲染出与 rect 一致的文本布局（T13 设备验证）
- [ ] resize/重建 surface 后恢复；surface lost 自动重配不崩（T13 设备验证）
**Verification:** `cargo test -p velm render`；设备截图；旋转/后台恢复验证
**Dependencies:** T2、T9（View 类型可先用最小本地定义，后接 T4）
**Files:** `crates/velm/src/render/{mod.rs,scene.rs,vello_renderer.rs,font.rs}`、`crates/velm/src/error.rs`
**Estimated scope:** M（5 文件）

> **实施记录（2026-09-22，T10 完成）**：
>
> - **渲染拆成两层**：`render::scene`（host 可见，纯逻辑：视图树 → `DrawCommand` 序列）+ `render::vello_renderer` / `render::font`（android-only）。这样「画什么」在 host 可单测，符合 §10.2；`render/mod.rs` 与 `engine/mod.rs` 同样做「按目标 gate 子模块」而非整块 cfg。
> - **`render/text.rs` 更名为 `render/font.rs`** 并加 `FontCache`（roboto + noto_sc、`shape_line`、`vertical_metrics`），与 PLAN 的文件清单对齐；垂直度量的符号归一（`descent` 取绝对值）放在 `FontFace::vertical_metrics`，纯几何 `centered_baseline` 放 `scene` 以便 host 测。
> - **`velm::Error` 落地**（thiserror 2）：只覆盖初始化路径 8 个变体；每帧绘制失败一律记日志跳过，不进控制流。
> - **偏离规格 1**：`VelloRenderer::new` 增加 `density` 参数（sp→px 需要，引擎持有 `ScreenConfig.density`）。**偏离规格 2**：`new` 内部先调 `window.configure_buffers`（RGBA_8888），缓冲几何是建 surface 的前置条件。均已回写 SPEC §7.8。
> - **T9 遗留验收项闭合**：rwh 0.6 句柄由 `NativeWindowWrapper` 导出后交给 `wgpu::create_surface_unsafe`（唯一接线点在 `new`）。
> - **顺带修掉 T9 标记的引用计数隐患**：新增 `NativeWindowWrapper::from_ndk_owned`（接手回调已 acquire 的引用，不再 acquire），引擎线程改为持有 `Option<NativeWindowWrapper>`，删除裸指针与 `release_window`；回调侧 acquire 保留（回调返回后框架可能回收其引用）。
> - **删除 T2 spike 的 `render_frame()` 探针帧**：首帧改由 T11 用 Activity 的真实视图树产出（T2 实证「不提交首帧则触摸不投递」，但探针帧是临时软件帧，不应带进正式渲染器）。
> - **实测坑**：wgpu 29 的 `request_adapter` 返回 `Result`（不是 `Option`，与 T2 spike 的 `.ok()?` 不同）；`CurrentSurfaceTexture` 无 `OutOfMemory` 变体，实际是 `Success/Suboptimal/Timeout/Occluded/Outdated/Lost/Validation`——规格里写的 `SurfaceError::OutOfMemory` 已按实际枚举改写成 §7.8 第 6 条。
> - 门禁：host/aarch64/x86_64 三目标 clippy `-D warnings` 全绿、android 双 target `cargo build --workspace` 通过、fmt 干净；host 单测 96（新增 9）。设备侧渲染/resize/surface-lost 行为待 T13 验证。

### Task 11: 静态画面接线（两泳道汇合）

**Description:** counter 实现 `Activity`（counter model、on_draw 三行文本 + 两个按钮矩形，ADR-10 视觉）；引擎初版：onCreate 启动引擎线程、WindowCreated 时建 renderer、`on_draw → measure → render` 出首帧；暂不接输入队列。
**Acceptance criteria:**
- [ ] 真机启动即见 SC-2 的静态画面（计数 0、+1/-1 按钮）
- [ ] 回前台/旋转后画面恢复
**Verification:** 截图比对；logcat 首帧日志
**Dependencies:** T8、T10
**Files:** `crates/velm/src/engine/activity_thread.rs`、`examples/counter/src/lib.rs`
**Estimated scope:** M

### Checkpoint C — 静态画面

- [ ] 真机稳定显示静态 UI；窗口重建路径可用
- [ ] 与人 review 渲染/布局坐标一致性（文本位置与 computed_rect 对齐）

## Phase 3 — TEA 交互闭环

### Task 12: 回调重写为事件发布 + 引擎线程 Looper

**Description:** 按 SPEC §3.3/§7.7、ADR-09/11 落地：5 个 C 回调只做 acquire/登记/channel 发布；引擎线程 Looper prepare、`AInputQueue_attachLooper`、pollOnce 唤醒；WindowDestroyed/QueueDestroyed 同步 ack（确认 renderer 停绘/不再 getEvent 后回调才返回）；onDestroy 发 Quit 并 join、释放 context；所有 FFI 入口 catch_unwind；非 motion 事件也保证 finishEvent 一次。
**Acceptance criteria:**
- [ ] 回调函数内无阻塞循环/无渲染调用（代码评审 + 日志时序）
- [ ] 启停 20 次：队列/窗口销毁回调均被执行、线程 join 成功、无 UAF/ANR
**Verification:** `adb` 压测脚本 + logcat 时序检查
**Dependencies:** T11
**Files:** `crates/velm/src/engine/{activity_thread.rs,app_context.rs}`、`crates/velm/src/platform/window.rs`（acquire/release 若 T9 未含）
**Estimated scope:** M

### Task 13: 输入 → hit-test → update → 重绘闭环

**Description:** 按 SPEC §7.7/§8.2 接线：事件取出→MotionEvent 解析→`on_touch_event` 拦截优先；否则 ActionDown 复用「当前帧已布局 View 树」做 hit-test（避免 docs 的一触双重建）；消息 VecDeque FIFO；逐个 update、合并一次重绘；measure(density) → render；MOVE/UP/CANCEL 仅走拦截路径；空白处不重绘。
**Acceptance criteria:**
- [ ] SC-3：点 +1/-1 计数正确，连点 20 次数字准确
- [ ] SC-4：空白无反应、按住不连发；拦截返回 Some 时不触发 hit-test
- [ ] 每个输入事件恰好 finishEvent 一次
**Verification:** 手工点击 + 录屏计数；logcat 消息/重绘日志核对
**Dependencies:** T12
**Files:** `crates/velm/src/engine/activity_thread.rs`、`crates/velm/src/engine/events.rs`（如需）
**Estimated scope:** M

### Checkpoint D — 端到端闭环

- [ ] SC-2/3/4 通过；交互路径 clippy/test 全绿

## Phase 4 — 硬化与交付

### Task 14: 生命周期压测与缺陷修复

**Description:** 编写 adb 脚本化压测：Home/返回/回前台、旋转各 ≥20 次，连续启停 100 次；验证 Model 跨窗口重建保留（SC-5）、引擎线程退出干净（SC-6）、无 surface/input 相关 abort 与 ANR；修复发现的问题。
**Acceptance criteria:**
- [ ] 压测脚本可重复运行且全程零崩溃/零 ANR；logcat 无 UAF/非法指针错误
- [ ] 回前台计数状态保留并可继续交互
**Verification:** `scripts/stress_lifecycle.sh` 输出与日志归档
**Dependencies:** T13
**Files:** `scripts/stress_lifecycle.sh`、按缺陷触及的 engine/render 文件（每轮修复控制范围）
**Estimated scope:** M

### Task 15: 打包配置与构建文档

**Description:** 固化 cargo-apk2 配置（应用名、minSdk 24、arm64，P1 x86_64）、`AndroidManifest.xml` 语义（NativeActivity、lib_name、hasCode）、release 构建；README 写清工具链（rustup target、NDK r27、cargo-apk2 安装）、构建/安装/logcat 命令链（SC-9，干净环境可复现）。
**Acceptance criteria:**
- [ ] 按 README 在干净 shell 中从零完成安装运行
- [ ] release APK 可安装启动
**Verification:** 严格照 README 手工走一遍
**Dependencies:** T14
**Files:** `examples/counter/Cargo.toml`（metadata）、`AndroidManifest.xml`（若 cargo-apk2 需要外置）、`README.md`、`scripts/build_run.sh`
**Estimated scope:** M

### Task 16: 质量门禁与文档归位

**Description:** 双 target clippy `-D warnings`、fmt、纯逻辑覆盖率 ≥85%、模块级 rustdoc（pub 项与 unsafe Safety 契约齐全）；SPEC/PLAN/DECISIONS 状态更新（决议落地勾选、P1 缺口登记 SC-10~13）；demo 视觉按 ADR-10 收尾（按压态除外）。
**Acceptance criteria:**
- [ ] SPEC §12 的 SC-1~SC-9 逐项有证据（命令输出/截图/日志）归档
- [ ] 三文档与最终代码一致；P1 项明确列入 v1.1
**Verification:** 全量门禁命令 + 交付说明
**Dependencies:** T15
**Files:** `crates/velm/src/**`（文档注释/小修）、`docs/SPEC.md`、`docs/PLAN.md`、`docs/DECISIONS.md`
**Estimated scope:** S

### Checkpoint E — v1 完成评审

- [ ] SC-1 ~ SC-9 全部满足且证据齐全
- [ ] 人 review 批准；进入 v1.1 backlog（SC-10~13：margin 精修/Intent 执行器/状态恢复/x86_64/文本精排/Choreographer）

---

## Risks and Mitigations

| 风险 | 级别 | 触发信号 | 缓解 |
|---|---|---|---|
| vello 0.10 Android API 与预期不符（surface 创建/文本 API 变动） | 高 | T2 无法在时间盒内出画面 | T2 是最高优先 spike、先于一切正式开发；失败则回 DECISIONS 评估 git 固定版/其他渲染路径，损失仅限 spike |
| glifo 0.2 与 vello 0.10 不配套 | 中 | T2 文本 POC 编译/运行失败 | 退回 skrifa 直绘（v1 单行文本足够），glifo 列入 v1.1 |
| raw-ndk-sys 缺 attachLooper/density 等符号 | 中 | T3 符号清单缺失 | 立即切 ndk-sys 0.6（已在 lock），ADR-02 已预留 |
| 销毁时序竞态（surface/queue 释放后访问） | 高 | T14 压测偶发崩溃 | T8 状态机单测覆盖乱序；T12 同步 ack；T14 专项压测 |
| wgpu 29 与其他依赖版本冲突 | 中 | T1/T10 依赖解析失败 | 以 vello 0.10 的 `^29.0.3` 为唯一基线，不混用 30 |
| cargo-apk2 workspace 支持问题 | 中 | T1 打不出包 | ADR-04 预留 cargo-ndk + 手写打包回退；记录后继续 |
| 坐标系/density 偏差导致点击错位 | 中 | 真机点击与视觉偏移 | ADR-12 统一像素坐标；T6 单测 + 多分辨率真机验证 |
| panic 跨 FFI | 中 | 任何设备 abort | T12 catch_unwind 统一入口；禁回调 unwrap |
| 范围蔓延（ort/rstar/redb 回流） | 中 | 实现中出现 AI/存储诉求 | ADR-01 + SPEC Boundaries：另开 spec，不进 v1 |

## Open Questions（留给 spike/实现中回答，不阻塞计划批准）

1. ~~T2：glifo 0.2 还是 skrifa 直绘（ADR-06 时间盒）？NotoSansCJK `.ttc` 的 collection index 取值？~~ **已关闭（2026-09-22）**：定稿 skrifa 0.44 直绘（glifo 面向 vello_common 新栈、不接 vello0.10 Scene）；NotoSansCJK ttc **index 2 = SC**（0 JP/1 KR/2 SC/3 TC/4 HK），见 render-poc §4 与 ADR-06。
2. ~~T3：raw-ndk-sys 符号清单结果；若切 ndk-sys，事件常量类型差异清单？~~ **已关闭（2026-09-22）**：符号全部具备，不切 ndk-sys；类型差异已记录（事件常量 u32 vs getter i32、getAction i32），见 spike §4。
3. ~~T10：vello 0.10 在 Android 的 surface 格式/呈现模式实测组合？~~ **已由 T2 实证关闭**：surface 强制非 sRGB `Rgba8Unorm`（避免 vello 输出被二次编码）、present mode 强制 Fifo、limits `Limits::default()`；vello 经 Rgba8Unorm 中间纹理 + TextureBlitter 上屏，见 render-poc §3。
4. ~~T15：cargo-apk2 对 workspace 内 cdylib package 的具体 metadata 字段？~~ **已由 T1 实证关闭**（见 `docs/spikes/2026-09-ndk-capabilities.md` 第 2 节）。

## 计划出口检查（planning skill）

- [x] 每个任务有验收条件与验证步骤
- [x] 依赖关系明确、高风险任务前置（T2/T3）
- [x] 无超过 5 文件的任务（最大 M=5）
- [x] 每 2~3 个任务设检查点（A~E），其中 CP-A/CP-C/CP-E 需人评审
- [x] 标注并行泳道与必须串行列
- [ ] 人类 reviewer 批准本计划后进入 TASKS 阶段（逐任务展开为可执行任务卡）
