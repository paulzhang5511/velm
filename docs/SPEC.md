# Spec: Velm-UI（Vello + Elm）Android 原生 2D GUI 框架



* **规格版本**：v1.0（SPECIFY 阶段产物）

* **创建日期**：2026-09-21

* **需求来源（权威输入）**：


  * `docs/prd_v1.md`（PRD v1.0，Zero-Glue Edition）

  * `docs/架构设计文档.md`（架构设计 v2.0，Zero-Glue Edition）

* **现状基线**：仓库尚无任何 commit；`src/main.rs` 仅为 `Hello, world!`；`Cargo.toml/Cargo.lock` 已存在且依赖版本与 docs 中的代码示例存在系统性偏差（见 §4、§13）。

* **文档约定**：全文用 **【PRD】**、**【ARCH】**、**【现状】**、**【规格】** 四种标注区分信息来源，避免把推断当成既定需求。



***

## 0. 假设（ASSUMPTIONS — 动手实现前必须先确认）

按 Spec-Driven Development 流程，以下为规格作者在 docs 未明确处做出的假设。**请在 review 时逐条确认或纠正；未被否认的假设将在 PLAN 阶段默认生效。**

> **状态更新（2026-09-21）**
>
> ：下列假设已经过取证与决策，全部转为正式决议，见 
>
> `docs/DECISIONS.md`
>
> （ADR-01 ~ ADR-12）与 
>
> `docs/PLAN.md`
>
> 。下文原文保留作为决策溯源；任何冲突以 DECISIONS/PLAN 为准。关键定稿：workspace 划分（ADR-07）、
>
> **vello 0.10 正式版 + wgpu 29**
>
> （ADR-03，替代假设 2 的 git 快照 /wgpu30）、移除 ort/rstar/redb/jni 等（ADR-01）、
>
> **minSdk 24**
>
> （ADR-08，替代假设 3 的 28）、cargo-apk2 打包（ADR-04）、文本栈 spike 定稿（ADR-06）。



1. **【规格】交付形态**：框架以库 crate 交付，同时产出 `cdylib`（供 Android 打包为 `.so`）与 `rlib`（供 host 单元测试与 Rust 侧复用）；PRD 中的计数器 Demo 作为 `examples/counter.rs`（或 `src/bin/counter.rs`）存在，而不是把业务代码写死在 `lib.rs`。当前 `src/main.rs` 的 bin 形态视为脚手架，最终移除或改为 demo。

2. **【规格】版本以仓库现状为准、docs 代码需迁移**：`raw-window-handle` 以现状 `0.6` 为准（docs 示例为 0.5 的 `HasRawWindowHandle` API，在 0.6 下无法编译）；渲染栈以现状 `vello_gpu` / `vello_common`（git，解析为 0.2.0 系列）+ `wgpu 30` 为准（docs 示例的 `vello = "0.2"` 元 crate 已不存在于 lock 中）。功能需求以 docs 为权威，**API 形态以现状依赖为准重写**。

3. **【规格】最小支持平台**：Android 单平台、`aarch64-linux-android`（arm64 真机）为首要目标；minSdk 取 28（Android 9），依据是【现状】`ort` 依赖启用了 `api-28` feature。`x86_64` 模拟器目标列为 P1。

4. **【规格】**`ort`**&#x20;/&#x20;**`rstar`**&#x20;/&#x20;**`redb`**&#x20;/&#x20;**`jni`**&#x20;不属于 v1 UI 框架范围**：这些【现状】依赖在两份 docs 中均无对应需求，本规格不据其虚构功能，仅登记为开放问题（§14 Q1）。`crossbeam-channel`、`pollster`、`bytemuck`、`image`、`rayon`、`serde*` 视为渲染 / 基础设施储备依赖，可在 PLAN 阶段决定去留。

5. **【规格】布局先手写、不引入 taffy**：【ARCH】目录与依赖清单写了 `taffy = "0.3"`，但【现状】lock 中无 taffy，且【ARCH】`measure.rs` 给的是手写线性布局。v1 以手写 LinearLayout 测量为准，taffy/Flexbox 完整能力列为后续演进项（§14 Q5）。

6. **【规格】事件循环必须独立线程**：docs 代码在 `onInputQueueCreated` 回调中直接进入死循环（见 §11.2 风险 R1），会阻塞 NDK 主回调线程，导致窗口 / 队列销毁回调永远无法执行。规格要求改为「回调只发布事件、独立引擎线程消费」的模型（对标 `native_app_glue` 模式）。这是对 docs 代码缺陷的**强制性修正**，而非可选优化。

7. **【规格】渲染器模块 v1 必须落地最小闭环**：【ARCH】目录中有 `render/vello_renderer.rs` 但无任何代码。v1 的定义是「能把布局后的 TextView 树通过 vello 绘制到窗口并 present」，否则框架不构成可验收产品。

8. **【规格】文本 shaping 方案未定**：【现状】仅有 vello 传递依赖带入的 `skrifa 0.44`，无 `parley`/`cosmic-text`/`swash`。v1 文本绘制方案（skrifa 直绘 vs 引入 parley）列为开放问题（§14 Q6），但成功标准要求英文文本可正确显示。

9. **【规格】语言与命名**：框架代码、标识符、日志使用英文（对齐 Android/NDK 术语）；文档使用中文，与 docs 现状一致。

10. **【规格】本规格只覆盖 SPECIFY 阶段**：PLAN（技术实现计划）、TASKS（任务拆解）、IMPLEMENT（编码）为后续三个独立阶段，需各自 review，不在本次交付内。



***

## 1. Objective（目标）

### 1.1 我们在构建什么

**Velm-UI** 是一个仅运行于 Android 原生 `NativeActivity` 之上的轻量级、声明式 2D GUI 框架，用 Rust 2024 edition 编写。两根支柱：



* **零胶水层（Zero-Glue）**：不依赖 `android-activity` 等跨平台胶水 crate（【现状】lock 已验证无 `android-activity`/`ndk` 高层 crate，仅有 `raw-ndk-sys`/`ndk-sys` 裸绑定）。框架直接导出 C-ABI 入口 `ANativeActivity_onCreate`，手动接管 `ANativeActivityCallbacks`，通过 `ALooper` + `AInputQueue` 自行驱动事件循环。

* **Elm 架构（TEA, The Elm Architecture）**：以强约束的单向数据流组织应用 ——`on_create`（初始化 Model）→ `on_draw`（Model → View 树）→ 事件产生 `Message` → `update`（Message → 新 Model + 副作用）→ 重绘，保证 UI 与状态一致。

### 1.2 要解决的痛点【PRD §1.1】



1. **跨语言开销**：传统 JVM UI（Android View / Jetpack Compose）或 JNI Native 方案存在高频跨语言调用与 GC 停顿。

2. **胶水层冗余**：`android-activity` 等抽象隐藏了 `ANativeActivityCallbacks` 与 `ALooper`，损失控制灵活性并带来体积开销。

### 1.3 设计目标【PRD §1.2】



| #  | 目标                | 含义                                                                                                                     |
| -- | ----------------- | ---------------------------------------------------------------------------------------------------------------------- |
| G1 | 零胶水、零 JNI 业务开销    | 仅依赖 `raw-ndk-sys` 与 `raw-window-handle` 对接平台；业务逻辑与渲染全程在 Rust 侧                                                         |
| G2 | Android 概念 1:1 对齐 | API、结构体、节点、事件名映射 Android 原生习惯（`Activity`/`View`/`ViewGroup`/`TextView`/`MotionEvent`/`LayoutParams`/`OnClickListener`） |
| G3 | 响应式单向数据流          | TEA 闭环：`on_create → update → on_draw`，状态唯一可信来源                                                                         |
| G4 | Rust 2024 规范      | edition = "2024"，高标准类型安全；所有裸指针解引用限定在显式 `unsafe` 边界内并标注 `# Safety` 契约                                                   |
| G5 | GPU 矢量渲染          | 基于 vello（wgpu 后端）绘制 View 树                                                                                             |

### 1.4 目标用户与用户故事



* **用户 A：Rust 嵌入式 / 图形开发者**，希望不用 JVM、不写 JNI 就能在 Android 上画出交互界面。

* **用户 B：Android 原生开发者**，熟悉 `Activity`/`View`/`MotionEvent` 概念，希望以最低学习成本使用 Rust 渲染 UI。

**用户故事（v1）**：



* US-1：作为开发者，我实现一个 `Activity` trait（含 `on_create`/`update`/`on_draw`），即可在 Android 设备上显示一个全屏竖排界面。

* US-2：作为开发者，我用链式调用 `View::text_view(...).set_text_size(...).set_text_color(...).set_on_click_listener(msg)` 构建文本与按钮。

* US-3：作为终端用户，我点击「+1」/「-1」区域，计数器文本立即更新（PRD 计数器 Demo 的端到端闭环）。

* US-4：作为开发者，我在旋转屏幕、切后台 / 前台、销毁 Activity 时不会崩溃、不死锁、无 UAF。

* US-5：作为开发者，框架的生命周期、输入、窗口、渲染各层都有日志，可在 logcat 中以 tag 过滤调试。

### 1.5 范围（v1）

**In Scope**



* NativeActivity 生命周期回调接管（窗口创建 / 销毁、输入队列创建 / 销毁、Activity 销毁）。

* TEA 三契约：`Activity` trait、`View<Msg>` 树、`Message` 队列与 `update` 调度。

* 基础视图：`TextView`、`ViewGroup`（仅 LinearLayout 的 Horizontal/Vertical 两种方向）。

* LayoutParams：`MATCH_PARENT` / `WRAP_CONTENT` / `Dp(f32)`、四向 margin（margin 字段必须在布局中生效，修正 docs 算法遗漏，见 §8.3）。

* 单点触控事件解析（DOWN/MOVE/UP/CANCEL）与 DFS Hit-Testing 点击分发。

* vello/wgpu 渲染：文本、基础色块、按 computed rect 摆放。

* 计数器 Demo 与 Android 打包清单（`AndroidManifest.xml`）、构建 / 安装命令。

**Out of Scope（v1 不做，除非 §14 开放问题另有决议）**



* 多点触控、手势识别、键盘 / 手柄 / IME 输入、传感器。

* 除 LinearLayout 外的布局（FrameLayout/RelativeLayout/ConstraintLayout/Flexbox 完整语义）。

* 图片、矢量图、动画、转场、裁剪、padding/gravity/weight（**按钮圆角矩形背景除外**，见 ADR-10）。

* `Intent` 异步副作用任务的真正执行器（v1 仅保留占位类型与 `none()`，见 §7.6；线程池异步为 P1）。

* `SavedInstanceState` 的真正持久化与恢复（v1 签名保留，恢复路径传 `None`，P1）。

* ONNX 推理（`ort`）、空间索引（`rstar`）、本地数据库（`redb`）、JNI 互操作（`jni`）——docs 无需求。

* iOS / 桌面跨平台。



***

## 2. 概念映射（Android ↔ Velm）【PRD §2.1】



| Android 原生概念                  | Velm 中的 Rust 形态                                        | 职责                                                          |
| ----------------------------- | ------------------------------------------------------ | ----------------------------------------------------------- |
| `Activity`                    | `trait Activity`                                       | 页面 / 生命周期容器：`on_create`、`update`、`on_draw`、`on_touch_event` |
| `View`                        | `enum View<Msg> { TextView(..), ViewGroup(..) }`       | 所有 UI 节点的统一抽象，持有样式与链式事件方法                                   |
| `ViewGroup`                   | `struct ViewGroup<Msg>`                                | 容器节点：`orientation` + `children: Vec<View<Msg>>`             |
| `TextView`                    | `struct TextView<Msg>`                                 | 文本节点：`text`、`text_size`、`text_color`                        |
| `MotionEvent`                 | `struct MotionEvent { action, x, y }`                  | 对齐 `ACTION_DOWN/MOVE/UP/CANCEL` 语义                          |
| `ViewGroup.LayoutParams`      | `struct LayoutParams` + `enum LayoutDimension`         | `MatchParent` / `WrapContent` / `Dp(f32)` + margin          |
| `View.OnClickListener`        | `.set_on_click_listener(msg: Msg)`                     | 以「发送一个 Message」作为点击回调                                       |
| `View` 的 mFrame/mLeft..mTop.. | `struct Rect { x, y, width, height }`（`computed_rect`） | 布局计算后的绝对绘制 / 命中区域                                           |
| Elm `Cmd`（副作用）                | `struct Intent<Message>`（v1 占位）                        | `update`/`on_create` 返回的异步任务集合                              |
| `ActivityThread` / 主 Looper   | `engine::activity_thread`                              | 回调绑定 + 事件循环 + 渲染调度                                          |



***

## 3. 系统架构

### 3.1 分层拓扑【ARCH §2，按规格修正】



```
┌──────────────────────────────────────────────────────────────────────┐

│ 1. Application Layer（开发者代码，examples/counter）                  │

│    MainActivity: impl Activity                                       │

│    Model(counter) ──on\_draw──► View Tree ──update◄── Message         │

└───────────────────────────────────┬──────────────────────────────────┘

&#x20;                                   │ View\<Msg> / Msg

┌───────────────────────────────────▼──────────────────────────────────┐

│ 2. Framework Core（纯 Rust，host 可测部分以虚线标出）                  │

│  ┌────────────────────┐   ┌─────────────────────┐  ┌───────────────┐  │

│  │ event::MotionEvent │──►│ engine::hit\_test    │  │ view::\*       │  │

│  │ (NDK AInputEvent → │   │ (DFS 命中→Msg) \~\~\~\~ │  │ View 树/Params│  │

│  │  MotionEvent)      │   └─────────┬───────────┘  └───────┬───────┘  │

│  └────────────────────┘             │                        │        │

│       ▲ FFI 边界                    ▼                        ▼        │

│  ┌────┴───────────────┐   ┌─────────────────┐      ┌───────────────┐  │

│  │ engine::           │   │ Message Queue   │─────►│ layout::measure│ │

│  │ activity\_thread    │   │ (VecDeque\<Msg>) │      │ 测量→computed  │ │

│  │ 回调表 + 引擎线程  │   └─────────────────┘      │ rect (可 host  │ │

│  │ + ALooper 轮询     │            │               │  单测) \~\~\~\~\~\~  │ │

│  └────────────────────┘            ▼               └───────┬───────┘  │

│                          Activity::update → Model          ▼          │

│                          Activity::on\_draw → View ──► render::vello\_  │

│                                                       renderer       │

└──────────────────────────────────────────────────────────┬───────────┘

&#x20;                                                           │ Scene/帧

┌──────────────────────────────────────────────────────────▼───────────┐

│ 3. Platform Bridge                                                    │

│  platform::NativeWindowWrapper                                        │

│   ANativeWindow 裸指针 ──► raw-window-handle 0.6 句柄                  │

│   ──► wgpu 29 Surface ──► vello 0.10（peniko/skrifa）──► Hardware      │

└───────────────────────────────────────────────────────────────────────┘
```

`~~~~` 标记的模块（`hit_test`、`measure`、View 数据结构、action 映射纯函数）不依赖 Android 链接，必须在 host（macOS/Linux）上可单元测试；FFI 解析与引擎循环仅编译于 android target。

### 3.2 TEA 单向数据流



```
系统回调/触摸

&#x20;  │ AInputQueue → MotionEvent

&#x20;  ▼

on\_touch\_event 拦截？ ──是──► 返回 Msg

&#x20;  │否

&#x20;  ▼

perform\_hit\_test(ActionDown 坐标, 已布局 View 树) ──► Option\<Msg>

&#x20;  ▼

VecDeque\<Message>（消息队列）

&#x20;  ▼ 逐个弹出

Activity::update(\&mut self, msg) -> Intent   // Model 变更

&#x20;  ▼ 队列本轮清空后

Activity::on\_draw(\&self) -> View\<Msg>        // 重建 View 树

&#x20;  ▼

layout::measure\_and\_layout(root, w, h)       // 写 computed\_rect

&#x20;  ▼

render::vello\_renderer::render(root)         // 编码 vello Scene 并 present
```

不变量：



* **IV-1**：任何一帧绘制前，View 树必须经过 `measure_and_layout`，每个节点的 `computed_rect` 对应当前窗口尺寸。

* **IV-2**：`update` 只在引擎线程被调用；`on_draw` 在同一线程、且 `&self` 不可变借用期间不得有其它访问。

* **IV-3**：Message 按产生顺序（FIFO）消费；同一轮循环内多个 Message 只触发一次重绘（docs 代码以 `needs_draw` 合并，规格保留）。

* **IV-4**：View 树是一次性临时值（每帧重建、随帧丢弃），持久状态只存在于 Activity（Model）中。

### 3.3 线程模型（对 docs 死循环方案的强制修正）

【ARCH】代码在 `onInputQueueCreated` 回调里直接调用阻塞式 `drive_event_loop`。NDK 的 `ANativeActivityCallbacks` 均在应用主线程被调用，阻塞它会使 `onInputQueueDestroyed`、`onNativeWindowDestroyed`、`onDestroy`、配置变更回调全部饿死，造成队列 / 窗口销毁后继续访问（UAF）与无法退出。

**规格要求的模型：**



```
Android 主线程（NDK 回调）                 引擎线程（spawn，std::thread）

─────────────────────────                 ─────────────────────────────

onNativeWindowCreated(w)  ──发布 WindowCreated(window 指针，先 acquire)──►

onInputQueueCreated(q)    ──发布 QueueCreated(queue，先 AInputQueue\_attachLooper 到引擎线程 Looper)

onNativeWindowDestroyed   ──发布 WindowDestroyed（栅栏/channel 同步，等 GPU 资源释放）

onInputQueueDestroyed     ──发布 QueueDestroyed（同步，确保引擎不再 getEvent）

onDestroy                 ──发布 Quit ──join

&#x20;                                         loop {

&#x20;                                           ALooper\_pollOnce(超时, ...)

&#x20;                                           取事件 → 输入解析 / 窗口管理 / 退出

&#x20;                                           消费 Msg → update → on\_draw

&#x20;                                           → measure → render

&#x20;                                         }
```

要求：



* 回调函数**只做**：指针登记、必要的 NDK 所有权 acquire（如 `ANativeWindow_acquire`）、通过 `crossbeam-channel`（ADR-01 保留）发布事件、销毁路径上做同步确认；**禁止**在回调中执行渲染或阻塞循环。

* 引擎线程持有自己的 `ALooper`（`ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS)`），将 `AInputQueue_attachLooper` 绑定到该 Looper，以 Looper 唤醒替代 docs 的固定 16ms 睡眠轮询；帧节奏由 `ALooper_pollOnce` 超时 + 请求重绘标志共同控制（v1 可保留～16ms 超时作为兜底）。

* 窗口销毁与队列销毁必须与引擎线程同步（channel 往返 ack 或在回调中 `ALooper_wake` + join 约定点），保证渲染器 drop surface 后系统才真正释放窗口。

* `AppContext` 不再以 `Box::into_raw` 挂在 `ANativeActivity::instance` 后被各回调裸取裸改；改为：`instance` 存放一个由引擎持有 / 回调可安全读取的共享句柄（推荐 `Box<EngineHandle>`，内含 channel 发送端），`onDestroy` 时发 Quit 并 `join`，最后 `Box::from_raw` 释放。具体数据归属在 PLAN 阶段定案。

### 3.4 生命周期状态机



```
&#x20;           onCreate(C 入口)

&#x20;                │ bind callbacks, spawn engine

&#x20;                ▼

&#x20;       ┌─── WindowCreated ──► HasSurface（configure buffers RGBA\_8888,

&#x20;       │                         init wgpu surface/vello renderer, 首帧）

&#x20;       │                         │

&#x20;       │      QueueCreated ──────┼──► Pumping（可输入 → hit-test → update）

&#x20;Running ┤                         │

&#x20;       │      QueueDestroyed ────┼──► 停止取输入（channel 同步）

&#x20;       │      WindowDestroyed ───┘──► 丢弃 surface/renderer（同步, 禁止再 present）

&#x20;       │   （窗口可反复创建/销毁：回到等待 WindowCreated）

&#x20;       └─── Destroy(Quit) ──► join 引擎线程 → 释放 context → 进程端清理
```



* 窗口尚未就绪或已销毁时到达的 Message / 重绘请求一律丢弃并打 warn 日志。

* 输入队列与窗口的到达顺序不保证（docs 隐式假设窗口先于队列），引擎必须独立处理两者的任意先后顺序。

**落地（T8 定稿）**：上述状态机实现为不依赖 Looper 的纯函数，位于 `engine::events`，host 可直接单测（§10.2）：

```
pub struct Viewport { pub width: i32, pub height: i32, pub density: f32 }

pub enum EngineEvent { WindowCreated(Viewport), WindowResized { width, height }, WindowDestroyed, QueueCreated, QueueDestroyed, Message, Redraw, Quit }

pub enum EngineAction { CreateSurface, ResizeSurface, DestroySurface, AttachQueue, DetachQueue, Exit }

pub struct EngineState { /\* window: Option\<Viewport>, queue: bool, quit: bool, needs\_draw: bool \*/ }

pub fn step(state: \&mut EngineState, event: EngineEvent) -> Vec\<EngineAction>;

impl EngineState { pub fn take\_draw\_request(\&mut self) -> bool; /\* viewport/has\_window/has\_queue/is\_quitting \*/ }
```

补充不变量（均有单测钉住，见 `tests/state_machine.rs`）：

* `EngineState` **不持有** ANativeWindow / AInputQueue 指针，只跟踪「有没有」和「尺寸是多少」；指针生命周期仍归 `activity_thread`。
* 重绘以**标志位**（`take_draw_request`）而非 `EngineAction::Redraw` 表达：引擎每帧取一次，取走即清零。
* 窗口销毁时**清除**尚未消费的重绘标志——无 surface 后不得再渲染。
* `WindowResized` 不携带 density（T3 回调只给宽高），沿用旧 density，禁止回落到 1.0。
* `Quit` 先回收仍持有的资源（`DestroySurface` → `DetachQueue`）再 `Exit`；置位后对任何事件都返回空动作集。
* 窗口 / 队列重复创建、重复销毁属异常路径：前者先销毁旧资源再重建（warn），后者忽略（warn），均不 panic。



***

## 4. 技术栈与依赖规格

### 4.1 依赖决策总表



| 能力           | docs 声明                                             | 仓库现状（Cargo.lock 实测）                                                                         | 规格决议                                                                                                                             |
| ------------ | --------------------------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| NDK 裸绑定      | `raw-ndk-sys = "0.1.2"`                             | `raw-ndk-sys 0.1.2` + 传递 `ndk-sys 0.6.0+11769913`                                           | **已定稿（ADR-02）**：raw-ndk-sys 0.1.2 为主，T3 spike 核对符号，缺则切 ndk-sys 0.6                                                               |
| 窗口句柄         | `raw-window-handle = "0.5.2"`（`HasRawWindowHandle`） | `0.6.2`                                                                                     | **以 0.6 为准**：实现 `HasWindowHandle`/`HasDisplayHandle`，返回 `Result<WindowHandle<'_>, HandleError>`；docs 代码需迁移                       |
| GPU 渲染       | `vello = "0.2"`、`peniko = "0.1"`                    | 曾解析为 git 快照 `vello_gpu/common` 0.2 开发期（rev 5c22cff）+ `wgpu 30`、`peniko 0.6.1`、`skrifa 0.44` | **已定稿（ADR-03）**：crates.io **vello 0.10 正式版 + wgpu 29**（vello 0.10 要求 `wgpu ^29.0.3`）、peniko 0.6、skrifa 0.44；放弃 git 快照（该形态已被上游重组） |
| 弹性布局         | `taffy = "0.3"`（ARCH 依赖表）                           | 缺失                                                                                          | **已定稿（ADR-05）**：v1 不引入，手写 LinearLayout                                                                                           |
| 异步副作用        | `pin-project = "1.1"`（PRD 依赖表）                      | 缺失；PRD 代码中的 `pin_project::UnpinFuture` 类型实际不存在                                              | v1 `Intent` 为占位空壳；真正任务器 P1，届时再定 executor 与 pin 依赖                                                                                |
| 日志           | `log 0.4` + `android_logger 0.13`                   | `log 0.4` + `android_logger 0.15.1`（仅 android target）                                       | 以 0.15 为准；host 测试无 logger                                                                                                        |
| 线程间消息        | 无                                                   | `crossbeam-channel 0.5.17`                                                                  | **保留（ADR-01）**：回调→引擎通道                                                                                                           |
| GPU 异步等待     | 无                                                   | `pollster 1.0.1`                                                                            | **保留**：wgpu future 阻塞执行                                                                                                          |
| 数据转换         | 无                                                   | `bytemuck 1.25`（derive）                                                                     | **保留**：GPU 数据转换（vello 生态同代）                                                                                                      |
| 错误类型         | 无                                                   | （vello 传递 thiserror 2.x）                                                                    | **新增（ADR-11）**：框架 `velm::Error` 使用 thiserror                                                                                     |
| 图像           | 无                                                   | `image 0.25`                                                                                | **移除（ADR-01）**：v1 无图片视图                                                                                                          |
| 并行           | 无                                                   | `rayon 1.12`                                                                                | **移除（ADR-01）**：v1 单引擎线程                                                                                                          |
| 序列化          | 无                                                   | `serde 1` + `serde_json 1`                                                                  | **移除（ADR-01）**：SavedInstanceState 已列 P1，届时按需引入                                                                                   |
| 空间索引         | 无                                                   | `rstar 0.12`                                                                                | **移除（ADR-01）**：docs 无需求，DFS 足够                                                                                                   |
| 嵌入式 KV       | 无                                                   | `redb 4.3`                                                                                  | **移除（ADR-01）**：docs 无需求                                                                                                          |
| JNI          | 无                                                   | `jni 0.22.4`（仅 android target）                                                              | **移除（ADR-01）**：违背零 JNI 定位                                                                                                        |
| ONNX Runtime | 无                                                   | `ort 2.0.0-rc.13`（api-28，仅 android）                                                         | **移除（ADR-01）**：docs 无需求，且会抬升 minSdk 与包体                                                                                          |

### 4.2 Cargo.toml 目标形态（ADR-03/07 定稿）

workspace 根清单只声明 members；框架库与 demo 各自独立：



```
\# 根 Cargo.toml

\[workspace]

members = \["crates/velm", "examples/counter"]

resolver = "2"

\# crates/velm/Cargo.toml —— 框架库（rlib，host 可测）

\[package]

name = "velm"

version = "0.1.0"

edition = "2024"

\[lib]

crate-type = \["rlib"]

\[dependencies]

log = "0.4"

raw-window-handle = "0.6"

peniko = "0.6"

vello = { version = "0.10", default-features = false, features = \["wgpu"] }

wgpu = "29"

pollster = "1.0"

crossbeam-channel = "0.5"

bytemuck = { version = "1.25", features = \["derive"] }

thiserror = "2"

\# 文本栈按 ADR-06 spike 结论二选一：glifo = "0.2" 或仅用 vello 自带 skrifa 0.44

\[target.'cfg(target\_os = "android")'.dependencies]

raw-ndk-sys = "0.1.2"   # spike 验证符号，缺则改 ndk-sys = "0.6"（ADR-02）

android\_logger = "0.15"

\# examples/counter/Cargo.toml —— demo（cdylib，NativeActivity 加载）

\[package]

name = "counter"

version = "0.1.0"

edition = "2024"

\[lib]

crate-type = \["cdylib"]

\[dependencies]

velm = { path = "../../crates/velm" }

peniko = "0.6"

log = "0.4"

\[target.'cfg(target\_os = "android")'.dependencies]

raw-ndk-sys = "0.1.2"

android\_logger = "0.15"
```

### 4.3 工具链（ADR-04/08 定稿）



* Rust：stable，**edition 2024**；本机实测 1.95.0（满足 vello 0.10 /glifo MSRV）。

* Android 目标：P0 `aarch64-linux-android`（arm64-v8a）；P1 `x86_64-linux-android`（模拟器）；不做 armeabi-v7a。

* **minSdk 24**（Android 7.0，wgpu/Vulkan 事实门槛；移除 ort 后无 28 约束）。

* NDK：**r27.3.13750724**（本机已装；r26.3 备用）。

* 打包：**cargo-apk2（≥1.4，无 Gradle）** 完成编译→APK→安装；`cargo-ndk`（本机已装）用于仅产出 `.so`/CI。需提供 NativeActivity 清单配置（`android.app.NativeActivity`、meta-data `android.app.lib_name=counter`）与可复现命令链；未来上商店需要 AAB 时再评估 xbuild/Gradle。



***

## 5. Commands（完整可执行命令）

> 下列命令为规格约定的权威入口（ADR-04/07/08 定稿）；NDK r27 路径与 cargo-apk2 元数据在 PLAN T1/T15 实测后固化到 README。



```
\# 工具链准备（本机已具备：rustc 1.95、android targets、NDK r27）

rustup target add aarch64-linux-android            # P1：x86\_64-linux-android

cargo install cargo-apk2                           # ADR-04 打包工具（无 Gradle）

\# 主机开发检查（纯逻辑模块必须在 host 编译通过）

cargo build --workspace

cargo test --workspace                             # layout / hit\_test / view / event / 状态机

cargo fmt --all -- --check

cargo clippy --workspace --all-targets -- -D warnings

cargo clippy --workspace -t aarch64-linux-android -- -D warnings   # android target 零 warning

\# Android 构建（demo cdylib）

cargo build -p counter --target aarch64-linux-android --release

\# 打包安装到真机/模拟器（cargo-apk2；子命令以 T1 实测为准）

cargo apk2 run -p counter --target aarch64-linux-android

\# 日志

adb logcat -s VelmEngine VelmActivity AndroidRuntime    # 观察生命周期/输入/渲染 tag
```



* **Build**：`cargo build --workspace`（host）/ `-p counter -t aarch64-linux-android`（设备）

* **Test**：`cargo test`（host）；设备侧集成测试见 §10。

* **Lint**：`cargo clippy --all-targets -- -D warnings`（双 target）

* **Format**：`cargo fmt --all`

* **Dev/Run**：打包安装 + `adb logcat`



***

## 6. Project Structure（目标目录结构）

以【ARCH §1】目录树为基线、按 ADR-07 拆为 workspace（框架 rlib + demo cdylib），补齐 docs 中只出现名字、未给实现的模块：



```
velm/

├── Cargo.toml                       # \[workspace] members = crates/velm, examples/counter

├── Cargo.lock

├── README.md                        # 构建/安装命令链（SC-9，T15）

├── scripts/                         # 构建与生命周期压测脚本（T14/T15）

├── docs/

│   ├── prd\_v1.md                    # 需求输入（已存在）

│   ├── 架构设计文档.md               # 架构输入（已存在）

│   ├── SPEC.md                      # 本规格

│   ├── DECISIONS.md                 # ADR-01\~12（决策记录）

│   ├── PLAN.md                      # 实现计划与任务拆解

│   └── spikes/                      # T2/T3 技术验证结论

├── crates/

│   └── velm/                        # 框架库（crate-type = \["rlib"]，host 可测）

│       ├── Cargo.toml

│       └── src/

│           ├── lib.rs               # pub mod 声明、re-export 公共 API（不含业务/入口）

│           ├── error.rs             # velm::Error（thiserror，ADR-11）

│           ├── app/

│           │   ├── mod.rs

│           │   ├── activity.rs      # Activity trait、Intent（v1 占位）

│           │   └── state.rs         # 运行时状态/句柄类型

│           ├── engine/

│           │   ├── mod.rs

│           │   ├── events.rs        # 通道事件枚举 + 状态机纯函数（host 可测）

│           │   ├── app\_context.rs   # AppContext/EngineHandle（通道发送端、线程归属）

│           │   ├── activity\_thread.rs # C 回调绑定、引擎线程、Looper 事件循环（§3.3）

│           │   └── hit\_test.rs      # DFS 命中测试（host 可测）

│           ├── event/

│           │   ├── mod.rs

│           │   └── motion\_event.rs  # AInputEvent → MotionEvent；decode\_action 纯函数

│           ├── layout/

│           │   ├── mod.rs

│           │   └── measure.rs       # 手写 LinearLayout 测量（host 可测，margin/density）

│           ├── platform/

│           │   ├── mod.rs

│           │   └── window.rs        # ANativeWindow 封装（acquire/release）、rwh 0.6、density

│           ├── render/

│           │   ├── mod.rs

│           │   ├── font.rs          # 系统字体加载（Roboto/Noto CJK，ADR-06）

│           │   └── vello\_renderer.rs# wgpu29 surface + vello 0.10 Scene 编码与 present

│           └── view/

│               ├── mod.rs           # View 枚举与链式构造器 re-export

│               ├── params.rs        # LayoutParams、LayoutDimension、Orientation、Rect、EdgeInsets

│               ├── text\_view.rs

│               └── view\_group.rs

└── examples/

&#x20;   └── counter/                     # Demo 应用（crate-type = \["cdylib"]，NativeActivity 加载）

&#x20;       ├── Cargo.toml               # cargo-apk2 打包元数据（minSdk 24、arm64）

&#x20;       └── src/lib.rs               # MainActivity + #\[no\_mangle] ANativeActivity\_onCreate
```

模块职责边界：



* `platform` 是唯一接触 `ANativeWindow*` 与窗口几何的模块；

* `event` 是唯一接触 `AInputEvent*` 的模块；

* `engine` 负责调度与所有权，不写绘制细节；

* `render` 是唯一接触 wgpu/vello 的模块，输入为「已布局 View 树 + 窗口尺寸」，输出为帧；

* `view`/`layout`/`engine::hit_test` 不出现任何 android FFI 符号（host 可测的硬约束）。



***

## 7. 模块 API 契约

> 以下契约提炼自 docs 代码并补齐缺陷。签名以「契约」为准；docs 代码在 rwh 0.6 /vello 拆分版下需要改写。所有 
>
> `unsafe fn`
>
>  必须写 
>
> `# Safety`
>
>  段说明调用前提。

### 7.1 `platform::window` — NativeWindowWrapper

**职责**：安全包装 `ANativeWindow` 裸指针；配置缓冲几何；导出 rwh 0.6 句柄；在窗口销毁路径上管理所有权。



```
pub struct NativeWindowWrapper { /\* NonNull\<ANativeWindow>，owned/acquired 状态明确 \*/ }

impl NativeWindowWrapper {

&#x20;   /// # Safety

&#x20;   /// 调用时 ptr 必须有效、非空；调用方需保证在引擎线程消费窗口期间

&#x20;   /// 系统不会释放它（通过 ANativeWindow\_acquire/release 或销毁同步点保证，见 §3.3）。

&#x20;   pub unsafe fn from\_ndk(ptr: \*mut ANativeWindow) -> Option\<Self>;

&#x20;   pub fn as\_raw\_ptr(\&self) -> \*mut ANativeWindow;

&#x20;   pub fn size(\&self) -> (i32, i32);                 // ANativeWindow\_getWidth/Height

&#x20;   pub fn format(\&self) -> i32;

&#x20;   pub fn configure\_buffers(\&self, w: i32, h: i32) -> Result<(), i32>; // RGBA\_8888

}

// rwh 0.6（替代 docs 的 HasRawWindowHandle/HasRawDisplayHandle）：

impl HasWindowHandle for NativeWindowWrapper { /\* AndroidNdkWindowHandle \*/ }

impl HasDisplayHandle for NativeWindowWrapper { /\* AndroidDisplayHandle \*/ }

// Drop：若通过 acquire 持有，须 ANativeWindow\_release。
```

契约要点：



* `configure_buffers` 失败返回 NDK 负错误码，调用方必须记日志并进入「无 surface」状态，不得 panic。

* rwh 0.6 的 `window_handle()` 返回 `Result`，句柄内含生命周期，渲染器持有期间不得销毁窗口（由 §3.3 销毁同步保证）。

### 7.2 `view` — 视图树（params /text\_view/view\_group）



```
\#\[derive(Clone, Copy, Debug, PartialEq)]

pub enum LayoutDimension { MatchParent, WrapContent, Dp(f32) }

\#\[derive(Clone, Copy, Debug, PartialEq)]

pub enum Orientation { Horizontal, Vertical }

\#\[derive(Clone, Debug)]

pub struct LayoutParams {

&#x20;   pub width: LayoutDimension,

&#x20;   pub height: LayoutDimension,

&#x20;   pub margin: EdgeInsets,           // 规格建议：top/bottom/left/right 收敛为 EdgeInsets

}

// 默认：width=WrapContent, height=WrapContent, margin=0

\#\[derive(Clone, Copy, Debug, Default)]

pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }

impl Rect { pub fn contains(\&self, x: f32, y: f32) -> bool; }

/// v1 最小背景样式（ADR-10 按钮视觉）：纯色填充 + 圆角半径（px）

\#\[derive(Clone, Copy, Debug, Default)]

pub struct Background { pub color: Option\<peniko::Color>, pub corner\_radius: f32 }

pub struct TextView\<Msg> {

&#x20;   pub text: String,

&#x20;   pub text\_size: f32,               // 单位：sp/dp（布局时乘 density，ADR-12）

&#x20;   pub text\_color: peniko::Color,

&#x20;   pub background: Background,

&#x20;   pub layout\_params: LayoutParams,

&#x20;   pub on\_click\_listener: Option\<Msg>,

&#x20;   pub computed\_rect: Rect,

}

pub struct ViewGroup\<Msg> {

&#x20;   pub orientation: Orientation,

&#x20;   pub background: Background,

&#x20;   pub layout\_params: LayoutParams,

&#x20;   pub children: Vec\<View\<Msg>>,

&#x20;   pub on\_click\_listener: Option\<Msg>,

&#x20;   pub computed\_rect: Rect,

}

pub enum View\<Msg> { TextView(TextView\<Msg>), ViewGroup(ViewGroup\<Msg>) }
```

构造器（链式，所有权消费 self 返回 self，对齐 docs）：



* `View::linear_layout(orientation, children) -> View<Msg>`：根容器默认 width/height = MatchParent。

* `View::text_view(impl Into<String>) -> View<Msg>`：默认 `text_size=16.0`、`text_color=WHITE`、无背景、params 默认。

* `.set_text_size(f32)`、`.set_text_color(Color)`：仅对 TextView 生效（非 TextView 时 docs 实现为静默 no-op；规格保留但加 `debug_assert`/trace 日志，便于发现误用）。

* `.set_background(color, corner_radius)`：设置节点背景填充（按钮矩形，ADR-10）；TextView/ViewGroup 均可。

* `.set_on_click_listener(Msg)`：TextView/ViewGroup 均可绑定。

* `.set_layout_params(LayoutParams)`（T4 新增，补充规格）：覆盖宽高规格与四向 margin。`linear_layout` 默认 MatchParent×MatchParent，需要 `Dp`/`WrapContent` 的按钮等节点通过它设置。

约束：



* `Msg` 在构造 / 布局阶段不需要任何 bound；仅 hit-test 返回消息时要求 `Msg: Clone`（与 docs 一致）。

* 视图节点不实现 `PartialEq/Drop` 特殊语义；每帧随作用域释放（IV-4）。

* `EdgeInsets { left, top, right, bottom }` 单位为 dp，布局阶段乘 density；提供 `all(f32)` 构造四向等距，`Default` 为全零。

* 对非 TextView 调用 `set_text_size/set_text_color` 时以 `log::trace!` 记录后原样返回（**不启用 `debug_assert`**：debug 构建下 panic 会破坏 docs 的静默 no-op 语义，测试也需在 debug 下覆盖该路径）。

### 7.3 `event::motion_event` — 触控事件



```
\#\[derive(Clone, Copy, Debug, PartialEq, Eq)]

pub enum TouchAction { ActionDown, ActionMove, ActionUp, ActionCancel }

\#\[derive(Clone, Debug)]

pub struct MotionEvent { pub action: TouchAction, pub x: f32, pub y: f32 }

impl MotionEvent {

&#x20;   pub fn action(\&self) -> TouchAction;   // 对齐 docs 的 get\_action 命名风格

&#x20;   pub fn x(\&self) -> f32;

&#x20;   pub fn y(\&self) -> f32;

&#x20;   /// # Safety: event 非空、类型为 AINPUT\_EVENT\_TYPE\_MOTION、指针在调用期间有效。

&#x20;   pub unsafe fn from\_ndk(event: \*const AInputEvent) -> Option\<Self>;

}
```

实现约束（可测性重构）：



* 将**纯映射逻辑**抽为 host 可测函数：`fn decode_action(raw_action: u32) -> Option<TouchAction>`（处理 `& 0xff` 掩码与 DOWN/MOVE/UP/CANCEL），FFI 函数只负责 `AInputEvent_getType`、`AMotionEvent_getAction/getX/getY(pointer_index=0)` 后调用纯函数。**T5 定稿**：`decode_action` **在函数内部**先与掩码求交再匹配，故对已掩码 / 未掩码输入幂等；v1 只支持单点，`POINTER_DOWN(5)`/`POINTER_UP(6)` 及 HOVER/SCROLL/BUTTON 等一律返回 `None`（由引擎侧 finishEvent 后丢弃）。

* 常量的整数类型以 `raw-ndk-sys`/bindgen 实际绑定为准（docs 代码在 `as i32/as u32` 上存在可疑转换，实现时以编译为准，不保留无依据的强转）。

* v1 只读取 pointer index 0（单点）；多点触控 Out of Scope，但代码结构应允许后续扩展。

### 7.4 `layout::measure` — 测量与布局



```
/// 入口：以屏幕物理像素宽高为根约束，递归写满每个节点的 computed\_rect。

pub fn measure\_and\_layout\<Msg>(root: \&mut View\<Msg>, width\_px: f32, height\_px: f32, density: f32);

/// v1 文本宽度估算（可替换点）：P1 接入 skrifa 真实 advance 时只换本函数。

pub fn estimate\_text\_width(text: \&str, text\_size\_px: f32) -> f32;
```

算法契约（在【ARCH §4.3】手写算法基础上修正）：

0. **两遍 O(n) 算法**（T6 定稿）：第一遍 `measure` 自顶向下求宽高写入 `computed_rect`（x/y 置 0），第二遍 `place` 自顶向下写绝对坐标。容器 WrapContent 需要先拿到子节点尺寸，故求尺寸必须先于定位；两遍各遍历一次全树，不做重复递归。

1. 根节点原点 (0,0)，约束为整窗尺寸。**T6 定稿**：根节点沿用与子节点相同的规则——可用区 = 整窗尺寸扣除根节点自身四向 margin（margin 为 0 时即原点 (0,0)、约束为整窗）；否则 MatchParent 根节点会溢出屏幕。

2. TextView：

* width：MatchParent = 父内容宽；Dp (v)=`v*density`；WrapContent = 文本估算宽（docs 公式 `chars * size * 0.6` 仅为 ASCII 估算，规格允许 v1 使用该近似，但必须 clamp 到父内容宽；中文 / 字形真实度量依赖 §14 Q6 文本方案，P1 精修）。

* height：MatchParent = 父内容高；Dp (v)=`v*density`；WrapContent=`size*density*1.4`（行高）。

1. ViewGroup：

* MatchParent = 父约束；Dp (v)= 换算；**WrapContent 不得直接等于父尺寸**（docs 实现把容器 WrapContent 等同 MatchParent，属于缺陷）：v1 要求按主轴 / 交叉轴 children 与 margin 求和得到包裹尺寸；若实现成本在 v1 过高，至少对根 LinearLayout 之外的容器给出正确包裹，并在代码与本规格注明限制。

* 按 orientation 顺序排列子节点，主轴偏移量累加「子节点 computed\_rect 主轴尺寸 + 相邻 margin」；交叉轴起始位置考虑 margin。

1. **margin 必须生效**（docs 算法完全忽略四个 margin 字段）：子节点可用内容区 = 父 rect 内边距（v1 无 padding，仅 margin）扣除自身四向 margin。

2. Dp/sp→px 统一乘 `density`（ADR-12：`AConfiguration_fromAssetManager` + `AConfiguration_getDensity`，dpi/160.0；符号在 T3 spike 核对）；拿不到时兜底 1.0 并打一次 warn。

3. 输出坐标为物理像素、绝对坐标，与 `MotionEvent.x/y`（NDK 返回的是像素坐标）同一坐标系 —— 这是 hit-test 正确性的前提。

4. **尺寸非负**（T6 定稿）：任一节点的可用区被 margin 吃光时（`avail - margin < 0`）尺寸退化为 0，不产生负宽高；文本估算宽 clamp 到父内容宽，Dp / MatchParent 不再二次 clamp（显式尺寸按显式处理）。

5. **容器 WrapContent 内部的 MatchParent 子节点**（T6 定稿的 v1 限制）：以求值时父节点的可用区（上界）为准，不再做第二遍重排——即 Android 的「measure 两次」语义在 v1 不实现，需在代码与本规格注明。

6. **margin 是 dp，必须乘 density**（T7 发现并修复的 T6 缺陷，2026-09-22）：margin 与 Dp/sp 同属 dp 空间，但它的三处消费——位置偏移、子节点可用区扣减、主轴推进量——全部发生在物理像素空间，故每一处使用前都必须先乘 `density`。漏乘时 density=1 的结果完全一致，缺陷只在 density≠1 的设备上显现（外边距只有应有值的一半）。回归用例：`tests/layout.rs::margin_is_scaled_by_density`、`sibling_margins_are_scaled_by_density`。

### 7.5 `engine::hit_test` — 命中测试



```
/// DFS：自根向下；ViewGroup 先排除自身 rect 不包含的情况，

/// 再按 children 逆序（后添加者在上层）递归，最后回落到容器自身监听。

pub fn perform\_hit\_test\<Msg: Clone>(root: \&View\<Msg>, x: f32, y: f32) -> Option\<Msg>;
```

契约（与【ARCH §4.4】一致，补充边界语义）：



* 仅在 `TouchAction::ActionDown` 时触发点击分发（docs 事件循环如此规定；MOVE/UP 交给 `on_touch_event` 拦截路径）。

* 命中判定使用 §7.2 `Rect::contains`（闭区间）；边界重叠时子节点逆序优先。

* 不修改 View 树，返回消息的克隆。

* **输入坐标系一致性**：若未来引入 viewport 变换 / 刘海偏移，必须在调用前把 MotionEvent 坐标变换到布局坐标系。

* **模块可见性**（T7 定稿）：`hit_test` 属 §10.2 要求 host 可测的纯逻辑，因此 `engine` 模块**不能整块** `#[cfg(target_os = "android")]` 关闭；android-only 的 `activity_thread` 在**子模块**上单独 gate，`engine` 本身在 host 可见。

* **透明容器**（T7 定稿的语义澄清）：「回落到容器自身监听」发生在子节点探测的 `find_map` 之内——某个子节点若自身及后代都不产生消息（如纯布局容器、未绑监听），探测会继续落到**更下层的兄弟节点**，而不是停在该子节点。即：未绑监听的容器对点击是「透明」的；只有**绑了监听**的容器才会拦下点击。回归用例：`tests/hit_test.rs::transparent_container_passes_through_to_lower_sibling`、`container_with_listener_blocks_lower_sibling`。

### 7.6 `app::activity` — Activity 与 Intent



```
/// v1 占位：保留类型与 API 形状，tasks 永远为空。

pub struct Intent\<Message> { /\* v1: 空；PhantomData\<Message> \*/ }

impl\<Message> Intent\<Message> { pub fn none() -> Self { /\* ... \*/ } }

pub trait Activity: Sized + 'static {

&#x20;   type Message: Send + Clone + 'static;   // Send：引擎线程/通道要求（§3.3）

&#x20;   type SavedInstanceState;

&#x20;   fn on\_create(saved: Option\<Self::SavedInstanceState>) -> (Self, Intent\<Self::Message>);

&#x20;   fn update(\&mut self, message: Self::Message) -> Intent\<Self::Message>;

&#x20;   fn on\_draw(\&self) -> View\<Self::Message>;

&#x20;   /// 开发者自定义触控拦截；返回 Some(msg) 时消息入队、且不再走默认 hit-test。

&#x20;   fn on\_touch\_event(\&mut self, \_event: \&MotionEvent) -> Option\<Self::Message> { None }

}
```

契约：



* v1 中 `on_create` 恒以 `None` 调用；`update`/`on_create` 返回的 `Intent` 被引擎记录但不执行（trace 日志），不得因此报错。

* `Intent` 的 `Clone/Copy/Default/Debug` **手写实现**而非 derive：derive 会为 `Message` 加上相应约束，但 Intent 只持有 `PhantomData<Message>`，不该给使用者的消息类型加负担（T8 定稿）。

* **TEA 运行时**（T8 定稿）：`app::state::ActivityRuntime<A: Activity>` 持有 Model、待处理消息队列与重绘标志，把「消费消息 → update → 请求重绘」从引擎循环中剥离为纯逻辑：

```
impl\<A: Activity> ActivityRuntime\<A> {
    pub fn create() -> Self;                                  // 调 A::on\_create(None)
    pub fn enqueue(\&mut self, message: A::Message);           // 只入队，不立即 update
    pub fn drain(\&mut self) -> usize;                         // 逐条 update，返回处理条数
    pub fn on\_touch\_event(\&mut self, event: \&MotionEvent) -> bool;  // Some → 入队并返回 true
    pub fn view(\&self) -> View\<A::Message>;                    // on\_draw，每帧最多一次
    pub fn take\_draw\_request(\&mut self) -> bool;
}
```

  `enqueue` 与 `drain` 分离是为了保证「一帧内只因消息重建一次视图树」（§7.7）；`on_touch_event` 返回 `true` 表示已拦截，引擎必须跳过默认 hit-test（FR-I2）。

* P1：Intent 任务执行器（后台任务完成后把 Message 送回队列）、SavedInstanceState 存取（届时按需重新引入 serde，存储方式另议；redb 已按 ADR-01 移除）。

* PRD 中 `Vec<Box<pin_project::UnpinFuture<Message>>>` 为不存在的 API，禁止照抄。

### 7.7 `engine::activity_thread` — 入口、回调与事件循环

**C 入口（demo crate 导出，框架提供启动函数）：**



```
/// 由 examples/counter 的 cdylib 导出；框架侧提供 run\_native\_activity 泛型实现。

\#\[no\_mangle]

pub unsafe extern "C" fn ANativeActivity\_onCreate(

&#x20;   activity: \*mut raw\_ndk\_sys::ANativeActivity,

&#x20;   saved\_state: \*mut c\_void,

&#x20;   saved\_state\_size: usize,

);
```



* Rust 2024 要求 `extern "C" fn` 显式标注 `unsafe`（docs 代码已符合）。

* 入口对 `activity` 非空断言（保留 docs 的 `assert!`），随后：初始化 logger（tag `VelmEngine`）、调用 `A::on_create(None)`、分配引擎、注册回调表、启动引擎线程。

* 必须绑定的回调（v1）：`onNativeWindowCreated`、`onNativeWindowDestroyed`、`onInputQueueCreated`、`onInputQueueDestroyed`、`onDestroy`。其余回调（`onStart/onResume/onPause/onStop/onConfigurationChanged/onContentRectChanged/onWindowFocusChanged/onSaveInstanceState/onLowMemory/onTrimMemory`）v1 可留空但建议登记日志；保存 / 恢复在 P1。

**引擎主循环（引擎线程，伪契约）：**



```
loop {

&#x20;   ALooper\_pollOnce(timeout\_ms)              // 绑定 input queue 后可被输入/唤醒事件打断

&#x20;   while 有窗口/队列/退出事件: 按 §3.4 状态机处理（含销毁同步）

&#x20;   if 收到 Quit: 退出循环

&#x20;   while AInputQueue\_getEvent >= 0:

&#x20;       if preDispatch == 0:

&#x20;           MotionEvent::from\_ndk(event)

&#x20;             → Activity::on\_touch\_event；Some(msg) 入队

&#x20;             → 否则 ActionDown 时：取当前帧已布局 View 树做 perform\_hit\_test，命中入队

&#x20;       finishEvent(queue, event, 1)

&#x20;   while let Some(msg) = queue.pop\_front(): activity.update(msg); needs\_draw = true

&#x20;   if needs\_draw 或 窗口刚创建:

&#x20;       root = activity.on\_draw();

&#x20;       measure\_and\_layout(\&mut root, w, h, density);

&#x20;       renderer.render(\&root);              // 见 §7.8；无 surface 时跳过并 warn

}
```

性能 / 正确性约束：



* 不得每帧为 hit-test 而**额外**重建 View 树。docs 在 ActionDown 分支单独 `on_draw()` 一次、update 后又 `on_draw()`，一触可能重建两次。规格要求引擎在每帧缓存「最近一次已布局 View 树」（或在输入处理当帧确保先布局一次），hit-test 复用该树；View 树不跨帧用于渲染以外的用途时，至少保证同一轮内不重复重建。

* `AInputQueue_finishEvent` 必须在每条事件处理路径上恰好调用一次（包括非 motion 事件与解析失败事件）。

* 引擎线程 panic 不得跨 FFI unwind：按 ADR-11，C 回调与引擎线程入口统一 `catch_unwind`（error 日志后受控停引擎），回调内禁止 unwrap/expect。

### 7.8 `render::vello_renderer` — Vello 渲染器（docs 缺失，规格定义）

**技术基线（ADR-03/06/10）**：vello 0.10 正式版（wgpu feature，wgpu 29）、peniko 0.6；文本经 T2 spike 在 glifo 0.2 与 skrifa 0.44 直绘间定稿，系统字体 Roboto + NotoSansCJK。

**职责边界**：



1. 凭 `NativeWindowWrapper`（rwh 0.6）创建 `wgpu` Instance/Adapter/Device/Queue 与 Surface（`SurfaceTargetUnsafe::RawHandle`）；按窗口尺寸配置 `SurfaceConfiguration`（格式取表面能力中与 `RGBA_8888` 缓冲一致的 `Rgba8Unorm`/`Srgb` 变体；呈现模式优先 `Fifo`）。

2. 持有 vello 0.10 渲染器、编码缓存与字体缓存（`render::font`）；窗口尺寸变化时重新 configure。

3. `render(&mut self, root: &View<Msg>)`：先填充 `#121212` 背景；按 `computed_rect` 绘制节点 ——TextView 文本（text\_size/text\_color，中文经 Noto fallback）；按钮节点的圆角矩形底色（ADR-10 视觉基线，v1 内完成，不放 P1）；编码提交并 present（`pollster` 阻塞等待 wgpu future）。

4. 资源生命周期：surface/device 在窗口销毁时显式销毁并与回调线程同步（§3.3）；重建窗口时重新创建渲染器。

**接口形状（职责级；具体 vello 0.10 类型 / 方法名以 T2 spike 实测为准）：**



```
pub struct VelloRenderer { /\* wgpu device/queue/surface, vello renderer/编码缓存, 字体缓存 \*/ }

impl VelloRenderer {

&#x20;   pub fn new(window: \&NativeWindowWrapper, w: u32, h: u32) -> Result\<Self, velm::Error>;

&#x20;   pub fn resize(\&mut self, w: u32, h: u32);

&#x20;   pub fn render\<Msg>(\&mut self, root: \&View\<Msg>);   // 失败记日志/重配 surface，不 panic

}
```

> 错误类型为 
>
> `velm::Error`
>
> （ADR-11，thiserror）；
>
> `SurfaceError::Outdated/Lost`
>
>  时重配重试，
>
> `OutOfMemory/Outdated`
>
>  不可恢复时停引擎并上报。

### 7.9 框架 `lib.rs` 与 demo（ADR-07）



* `crates/velm/src/lib.rs` 只做模块声明与公共 re-export（`pub use app::{Activity, Intent}; pub use view::*; pub use event::...; pub use platform::...; pub use engine::run_native_activity`），**不含** `MainActivity` 与 `ANativeActivity_onCreate`。

* `examples/counter/src/lib.rs`（独立 cdylib package）：实现计数器（竖排：计数文本 28sp 白色；「+1」20sp 绿底圆角矩形、「-1」红底，ADR-10；分别绑定 `ClickIncrement/ClickDecrement`），并在该 crate 内导出 `#[no_mangle] unsafe extern "C" fn ANativeActivity_onCreate`，内部仅做非空断言后调用 `velm::engine::run_native_activity::<MainActivity>(...)`。



***

## 8. 功能行为规格（验收级）

### 8.1 生命周期与窗口



* FR-L1：安装启动后，logcat 依序出现 onCreate → 回调绑定成功 → onNativeWindowCreated（含宽高 / RGBA\_8888 配置成功）→ 首帧渲染日志。

* FR-L2：按 Home / 返回或切后台导致窗口销毁时，出现 onNativeWindowDestroyed 且进程不崩溃；回前台窗口重建后画面恢复、计数状态保留（Model 在引擎中存活）。

* FR-L3：旋转 / 配置变更导致窗口尺寸变化时，surface 重配、布局按新尺寸重算。

* FR-L4：退出 Activity 时 onDestroy → 引擎线程退出（join 返回）→ context 释放，无泄漏日志、无 ANR。

### 8.2 输入与交互



* FR-I1：手指按下「+1」文本区域（ActionDown 且坐标落在其 computed\_rect），计数 +1 并重绘；「-1」同理 -1。

* FR-I2：开发者重写 `on_touch_event` 返回 Some 时，该事件不再触发默认 hit-test。

* FR-I3：MOVE/UP/CANCEL 能被 `on_touch_event` 观察到；默认点击只在 DOWN 触发一次（不会因手指按住连续触发）。

* FR-I4：点击空白区域无任何消息产生、不重绘。

* FR-I5：每个从队列取出的事件都被 `finishEvent`，系统输入不会卡死。

### 8.3 布局与渲染



* FR-R1：竖排 LinearLayout 子视图从上到下依次排列，原点与间距符合 margin 设置。

* FR-R2：MatchParent 根容器铺满窗口；Dp 值在不同 density 设备上物理尺寸一致。

* FR-R3：三行 demo 文本以各自字号 / 颜色清晰可见，无裁切（WrapContent 高度容纳字号）。

* FR-R4：每帧画面与 Model 一致（计数数字准确反映点击次数）。

### 8.4 可观测性



* FR-O1：统一日志 tag（引擎 `VelmEngine`；建议 demo `VelmActivity`），关键路径（回调、buffer 配置、Looper 唤醒、消息、重绘、surface 生命周期）有 info/debug 级日志，异常路径 error 级含 NDK 返回码。



***

## 9. Code Style（代码风格）

### 9.1 代表性风格（一段真实风格的代码胜过三段描述）



```
//! engine/hit\_test.rs — 点击命中测试（host 可测，无任何 android 符号）

use crate::view::View;

/// 在已完成布局的 View 树中执行 DFS 命中测试，返回被点击节点绑定的消息。

///

/// - 容器自身 rect 不包含坐标时直接排除；

/// - 子节点按逆序探测，使后添加（绘制在上层）的子节点优先响应；

/// - 无子节点命中时，回落到容器自身的点击监听。

pub fn perform\_hit\_test\<Msg: Clone>(root: \&View\<Msg>, x: f32, y: f32) -> Option\<Msg> {

&#x20;   match root {

&#x20;       View::TextView(tv) => tv

&#x20;           .computed\_rect

&#x20;           .contains(x, y)

&#x20;           .then(|| tv.on\_click\_listener.clone())

&#x20;           .flatten(),

&#x20;       View::ViewGroup(vg) => {

&#x20;           if !vg.computed\_rect.contains(x, y) {

&#x20;               return None;

&#x20;           }

&#x20;           vg.children

&#x20;               .iter()

&#x20;               .rev()

&#x20;               .find\_map(|child| perform\_hit\_test(child, x, y))

&#x20;               .or\_else(|| vg.on\_click\_listener.clone())

&#x20;       }

&#x20;   }

}
```

### 9.2 规范条目



1. **Rust 2024 edition**；`unsafe extern "C"` 显式标注；禁止 `unsafe_op_in_unsafe_fn` 风格的隐式裸操作 —— 每个 unsafe fn 内解引用再包一层 `unsafe {}` 并配 `# Safety` 文档。

2. **unsafe 收敛**：裸指针解引用只允许出现在 `platform/`、`event/`、`engine/activity_thread` 三个模块；`view/layout/render 的编码逻辑/hit_test` 保持 safe Rust。每个跨 FFI 边界假设（指针有效性、线程归属、生命周期）写成注释契约。

3. **命名对齐 Android**：类型 / 方法沿用 docs 命名（`on_create`、`on_draw`、`set_on_click_listener`、`get_x` 风格 getter 可简化为 `x()`，但构造器与生命周期名保持 Android 映射）；模块 / 文件 snake\_case，类型 UpperCamelCase。

4. **Builder 链式**：视图配置方法消费并返回 `self`，不做静默失败以外的副作用；误用（对容器设字号）以 trace/debug\_assert 提示。

5. **错误处理**：NDK 返回码用 `Result<_, i32>` 或领域 error enum 传播；跨 FFI 边界不返回 Result、不 panic、不 unwind；可恢复的 GPU 错误（surface lost）重配重试，不可恢复错误记录后停引擎。

6. **日志**：`log` facade + android target 上 `android_logger::init_once`；模块前缀方括号（如 `[NativeWindowWrapper]`）；禁止热路径每帧 spam 级以上日志（逐帧日志用 Trace 且可关）。

7. **无循环依赖**：模块单向依赖 `app/view/event/layout/platform` ← `engine` ← `render` 协作；`view/layout` 不依赖 engine。

8. **格式化**：`rustfmt` 默认风格 + 每行宽度 ≤ 100；clippy 双 target `-D warnings`。

9. **泛型消息&#x20;**`Msg`：不在数据结构定义阶段强加 `Clone/Send` bound，只在真正需要的函数（hit\_test、引擎通道）声明最小 bound。

10. **文档注释**：所有 pub 项有 `///` 一行摘要；FFI 包装类型注明所有权与线程归属。



***

## 10. Testing Strategy（测试策略）

### 10.1 分层



| 层级                    | 对象                   | 位置 / 方式                                    | 覆盖内容                                                                         |
| --------------------- | -------------------- | ------------------------------------------ | ---------------------------------------------------------------------------- |
| 单元测试（host，cargo test） | `decode_action`      | `event/motion_event.rs` `#[cfg(test)]`     | 掩码 `&0xff`、DOWN/MOVE/UP/CANCEL、非法值 None                                      |
| 单元测试（host）            | `measure_and_layout` | `layout/measure.rs` 内测试或 `tests/layout.rs` | MatchParent/WrapContent/Dp、density 换算、margin、横 / 纵排列、clamp；用构造器建树后断言各节点 rect |
| 单元测试（host）            | `perform_hit_test`   | `tests/hit_test.rs`                        | 叶子命中 / 未命中、子节点逆序优先、容器回落、边界点                                                  |
| 单元测试（host）            | view 构造器             | `tests/view.rs`                            | 默认值、链式设置、no-op 行为                                                            |
| 集成测试（host）            | TEA 调度纯逻辑            | 抽出的「消息队列→update→重绘标志」纯函数（不依赖 NDK）          | FIFO、多消息合并一次重绘、拦截优先于 hit-test                                                |
| 设备集成测试（android）       | 引擎闭环                 | 真机 / 模拟器手动 + 脚本（adb）                       | §8 全部 FR；logcat 断言、截图比对（可选）                                                  |
| 冒烟                    | 启动 / 销毁压测            | adb 脚本反复启停 / 旋转 100 次                      | 无崩溃、无 surface 泄漏、无 ANR                                                       |

### 10.2 可测性设计要求（强制）



* `event` 模块把「NDK 取值」与「action 解码」拆分为 FFI 薄壳 + 纯函数（§7.3），否则 host 无法链接 `raw-ndk-sys`。

* 引擎把「通道事件 + 当前状态 → 动作（取事件 / 更新 / 重绘 / 退出）」的决策逻辑做成不依赖 Looper 的纯函数枚举，主循环只搬运，便于 host 单测状态机（§3.4）。

* 布局与 hit-test 不引入 `peniko::Color` 以外的平台类型；`Color` 在 host 可用（peniko 为纯 Rust）。

### 10.3 覆盖率与门槛



* v1 门槛：`layout`、`hit_test`、`event` 纯函数行覆盖 ≥ 85%；所有 pub 契约至少一个正例 + 一个边界 / 异常例。

* CI / 本地门槛：`cargo fmt --check`、双 target clippy `-D warnings`、host `cargo test` 全绿才可交付。

* 设备验收以 §13 成功标准清单逐项过；手工结果记录在 PR / 交付说明（命令、设备型号、Android 版本、logcat 摘要）。



***

## 11. Boundaries（边界）

### Always do（始终执行）



* 每次交付前跑 host `cargo test`、`cargo fmt --all -- --check`、双 target `cargo clippy -D warnings`。

* 所有跨 FFI 裸操作用 `unsafe fn` + `# Safety` 契约 + 内部 `unsafe {}` 块限定。

* 窗口 / 队列销毁走同步协议，确认引擎不再访问后才允许系统释放。

* 每个 AInputQueue 事件恰好 `finishEvent` 一次。

* 新增 pub API 先更新本规格再实现（spec is the source of truth）。

* 保留 docs 中的 Android 命名映射与链式 API 风格。

### Ask first（先确认再做）



* 改动 `Activity` trait 方法签名或消息 / 生命周期模型。

* 新增第三方依赖、把 git 依赖升级 / 换源、改动 crate-type。

* 引入 `ort`/`rstar`/`redb`/`jni` 相关任何功能代码。

* 改动目标平台矩阵（minSdk、目标 ABI、iOS / 桌面）。

* 把 taffy 或 parley 等大型依赖引入框架核心。

* 修改 `ANativeActivity_onCreate` 导出方式与打包结构。

### Never do（禁止）



* 禁止在 NDK 回调线程执行阻塞事件循环、渲染或 `join` 自身线程。

* 禁止引入 `android-activity` 等胶水层（零胶水是核心定位 G1）。

* 禁止在业务路径使用 JNI 往返（`jni` crate 不得进入框架核心调用路径）。

* 禁止让 panic 跨 C-ABI unwind；禁止在 FFI 回调中 `unwrap()`/`expect()`（断言仅允许用于入口空指针这类编程错误且需评估 abort 行为）。

* 禁止照抄 docs 中已确认的失效代码：rwh 0.5 API、`pin_project::UnpinFuture`、`vello = "0.2"` 元 crate、阻塞式 `drive_event_loop`、忽略 margin 的布局、容器 WrapContent=MatchParent。

* 禁止提交 secrets / 签名密钥；禁止改动 `target/` 与未授权的外部设备状态。



***

## 12. Success Criteria（成功标准，可测试）

v1 完成（Definition of Done）需**全部**满足：

**P0（不满足即未完成）**



* [ ] SC-1：`cargo build -p counter --target aarch64-linux-android` 产出含导出符号 `ANativeActivity_onCreate` 的 `libcounter.so`（`nm -D` 可查），cargo-apk2 打 APK 可安装到 arm64 真机（**minSdk 24**）并由 `android.app.NativeActivity` 加载启动。

* [ ] SC-2：启动后 logcat 出现完整生命周期链与首帧日志；屏幕显示深色背景（#121212）、计数文本、绿 / 红圆角按钮（ADR-10）；英文与中文文本均可正确渲染（ADR-06，换行 / 精排仍属 P1）。

* [ ] SC-3：点击「+1」/「-1」文本区域，计数以 1 为步长正确增减并即时重绘（连续点击 20 次数字准确）。

* [ ] SC-4：点击空白无反应；MOVE/UP 不产生重复点击。

* [ ] SC-5：反复 Home / 返回 / 回前台 / 旋转各 ≥ 20 次（脚本化），无崩溃、无 ANR、无 surface / 窗口相关 abort，回前台可继续交互且计数状态保留。

* [ ] SC-6：退出 Activity 后引擎线程确认退出（join 完成），logcat 无 UAF / 非法指针访问迹象。

* [ ] SC-7：host `cargo test` 全绿（layout/hit\_test/event/view/ 状态机），覆盖门槛 §10.3 达标；`fmt --check` 与双 target clippy `-D warnings` 零告警。

* [ ] SC-8：rwh 0.6 句柄实现正确，wgpu surface 按窗口尺寸配置并在尺寸变化时重配；帧实际 present（黑屏不算完成）。

* [ ] SC-9：`AndroidManifest.xml` + 一条从源码到安装运行的文档化命令链可在干净环境复现（README 或 docs 内）。

**P1（可在 v1.1 补齐，但需在交付说明中声明缺口）**



* [ ] SC-10：margin 在四方向上与验收用例一致；Dp 在多 density 设备物理尺寸一致。

* [ ] SC-11：`Intent` 可投递后台任务并把完成消息送回队列；`SavedInstanceState` 可保存 / 恢复计数。

* [ ] SC-12：`x86_64` 模拟器目标可用；按压态等视觉反馈。

* [ ] SC-13：真实文本度量（替换 `chars*size*0.6` 近似）与自动换行；更完整的字体 fallback。



***

## 13. 现状与目标差距矩阵



| 领域         | docs 目标                                         | 仓库现状                                   | 差距 / 动作                                                                              |
| ---------- | ----------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------ |
| crate 形态   | `velm-demo`，cdylib                              | `velm`，默认 bin（Hello world）             | **ADR-07**：转 workspace，框架 rlib（crates/velm）+ demo cdylib（examples/counter），删 main.rs |
| 源码         | 7 个模块完整设计                                       | 仅 `src/main.rs` 45 字节脚手架               | 按 PLAN T1\~T16 全部从零实现                                                                |
| 窗口句柄       | rwh 0.5 `HasRawWindowHandle`                    | rwh 0.6.2                              | 按 0.6 重写（§7.1）                                                                       |
| 渲染栈        | vello 0.2 元 crate + peniko 0.1                  | git 快照 vello\_gpu/common + wgpu 30     | **ADR-03**：改用 crates.io vello 0.10 + wgpu 29 + peniko 0.6                            |
| 渲染器本身      | ARCH 列了文件无代码                                    | 不存在                                    | 从零实现 §7.8（SC-8），T2 spike 先行                                                          |
| 布局         | 手写 LinearLayout（忽略 margin、无 density、容器 wrap 缺陷） | 不存在                                    | 实现并修正（§7.4），host 单测                                                                  |
| 命中测试       | DFS 代码完整                                        | 不存在                                    | 搬迁 + 单测（基本可直接用）                                                                      |
| 事件循环       | 回调内死循环 16ms 轮询                                  | 不存在                                    | 按 §3.3 重写为引擎线程 + Looper + channel                                                    |
| 生命周期       | 5 个回调                                           | 不存在                                    | 实现 + 销毁同步协议（ADR-11）                                                                  |
| Intent     | v1 占位 /v2 `Vec<()>`                             | 不存在                                    | 占位类型先落地，执行器 P1                                                                       |
| 状态保存       | trait 关联类型                                      | 不存在                                    | v1 传 None，P1                                                                         |
| 打包         | 提到 AndroidManifest.xml                          | 不存在                                    | **ADR-04**：cargo-apk2 + NativeActivity 配置（T1/T15）                                    |
| 测试         | 无                                               | 无                                      | 按 §10 新建                                                                             |
| 未文档化依赖     | —                                               | ort/rstar/redb/jni/image/rayon/serde 等 | **ADR-01**：v1 全部移除                                                                   |
| 文本 shaping | 未提                                              | skrifa 0.44（vello 传递）                  | **ADR-06**：T2 spike 定 glifo 0.2 /skrifa 直绘，系统字体，中英必达                                 |



***

## 14. Open Questions（已全部决议，2026-09-21）

> 决议全文见 
>
> `docs/DECISIONS.md`
>
> ；以下保留原问题作为决策溯源。



1. ~~Q1（范围）~~~~ ort/rstar/redb/jni 等~~ → **已决议（ADR-01）**：v1 全部移除（含 image/rayon/serde\*）；保留 crossbeam-channel/pollster/bytemuck。未来能力另开 spec。

2. ~~Q2（NDK 绑定）~~~~ raw-ndk-sys vs ndk-sys~~ → **已决议（ADR-02）**：raw-ndk-sys 0.1.2 为主，T3 spike 核对符号，缺则切 ndk-sys 0.6。

3. ~~Q3（vello 来源）~~~~ git pin 还是正式版~~ → **已决议（ADR-03）**：crates.io vello 0.10 + wgpu 29，放弃 git 0.2 开发快照。

4. ~~Q4（打包链）~~~~ cargo-apk/xbuild/Gradle~~ → **已决议（ADR-04）**：cargo-apk2（≥1.4，无 Gradle），cargo-ndk 备用；T1 验证 workspace 支持。

5. ~~Q5（布局引擎）~~~~ taffy~~ → **已决议（ADR-05）**：v1 手写 LinearLayout，不引 taffy。

6. ~~Q6（文本）~~~~ skrifa/parley、中文~~ → **已决议（ADR-06）**：T2 时间盒在 glifo 0.2 与 skrifa 0.44 直绘间定稿，默认跟随 vello 0.10 官方用法；中英文显示为 v1 必须项，系统字体 Roboto + NotoSansCJK。

7. ~~Q7（demo 位置与导出）~~ → **已决议（ADR-07）**：workspace = `crates/velm`（rlib）+ `examples/counter`（cdylib），删 `src/main.rs`。

8. ~~Q8（minSdk/ABI）~~ → **已决议（ADR-08）**：minSdk 24、NDK r27、arm64 P0、x86\_64 P1、不做 v7a。

9. ~~Q9（帧率模型）~~ → **已决议（ADR-09）**：v1 按需渲染 + Looper 唤醒（16ms 超时兜底）；AChoreographer 列 P1。

10. ~~Q10（视觉基线）~~ → **已决议（ADR-10）**：#121212 背景 + 圆角矩形按钮（+1 绿 /-1 红）。

衍生决议：错误处理与 FFI 安全边界（ADR-11，thiserror + catch\_unwind + acquire/release）、density 与像素坐标系统一（ADR-12）。

\*\* 仍待 spike 回答的次级问题（不阻塞计划批准）\*\* 见 `docs/PLAN.md`「Open Questions」（文本 API 细节、ttc index、NDK 符号清单、cargo-apk2 metadata）。



***

## 15. 风险登记（Risk Register）



| ID  | 风险                               | 影响             | 缓解                                                          |
| --- | -------------------------------- | -------------- | ----------------------------------------------------------- |
| R1  | 回调线程阻塞（docs 方案）                  | 销毁回调饿死、UAF/ANR | §3.3 引擎线程 + 同步销毁；PLAN T12/T14 专项实现与压测                       |
| R2  | vello 0.10 Android API 与预期不符     | renderer 返工    | **ADR-03 已定正式版**；PLAN T2 为最高优先 spike，先于一切正式开发，失败回 DECISIONS |
| R3  | ~~git 依赖不可复现~~ / 上游 API 变动       | 构建漂移           | 已弃 git 快照（ADR-03）；vello/wgpu 锁 semver 正式版，Cargo.lock 提交     |
| R4  | 文本栈（glifo/skrifa、CJK 字体）不确定      | 中文 / 度量返工      | ADR-06 时间盒 spike（T2）；skrifa 直绘兜底，glifo 列 v1.1               |
| R5  | 输入坐标与布局坐标系不一致（density/insets）    | 点击错位           | ADR-12 统一像素坐标；T6 单测 + 多分辨率真机验证                              |
| R6  | surface lost / 尺寸变化处理不当          | 切后台 / 旋转崩溃     | renderer 可重配设计；T14 生命周期压测 SC-5                              |
| R7  | FFI panic unwind                 | 未定义行为 /abort   | ADR-11 catch\_unwind 统一入口；禁回调 unwrap                        |
| R8  | `raw-ndk-sys` 符号 / 常量类型与 docs 不符 | 编译反复           | T3 符号清单核对，兜底 ndk-sys 0.6（ADR-02）；action 映射纯函数化              |
| R9  | 范围蔓延（已移除依赖回流）                    | v1 无法收敛        | ADR-01 + Boundaries：另开 spec，不进 v1                           |
| R10 | 无 CI / 设备矩阵说明                    | “我机器上能跑”       | T15 README 命令链（SC-9）、T14 压测脚本与结果留痕                          |



***

## 16. 后续阶段（进展）

按 Spec-Driven Development 门禁流程：



1. **SPECIFY**：本文件，已完成；开放问题已于 2026-09-21 全部决议（`docs/DECISIONS.md`）。

2. **PLAN**：**已产出** `docs/PLAN.md`—— 依赖图、T1\~T16 任务（spike 先行、逻辑 / 平台双泳道并行）、检查点 A\~E、风险与并行机会。**当前状态：待人类 review 批准**。

3. **TASKS**：计划批准后，逐任务展开为可执行任务卡（验收条件、验证命令、文件清单已在 PLAN 中具备雏形）。

4. **IMPLEMENT**：依任务 TDD / 增量实现，决策变化时先改规格与 ADR。

**Review 检查清单（SPECIFY 出口）**



* [x] 覆盖六大核心领域：Objective / Commands / Project Structure / Code Style / Testing Strategy / Boundaries

* [x] 成功标准具体、可测试（§12）

* [x] 假设前置且逐条可否认（§0）

* [x] 开放问题与风险显式登记并全部决议（§14/§15 + DECISIONS.md）

* [x] docs 与现状代码的差异全部记录、未静默吞掉（§4/§13）

* [x] PLAN 已产出并与本规格一致（docs/PLAN.md）

* [ ] 人类 reviewer 审阅并批准 SPEC + DECISIONS + PLAN（**当前状态：待 review**）