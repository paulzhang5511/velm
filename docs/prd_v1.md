# Velm (Vello + Elm) 详细需求设计文档 (v1.0 Zero-Glue Edition)

**项目名称**：Velm (Vello + Elm)

**全称**：Velm-UI: A Native Android 2D GUI Framework powered by Vello & The Elm Architecture

**核心定位**：基于 Rust `vello` GPU 矢量渲染引擎与 Android 原生 `NativeActivity` 构建的轻量级、声明式、**零胶水层（不依赖 `android-activity`）** 的 Android 原生 UI 框架。**采用 Elm 架构（TEA）进行状态驱动，全套 API 结构体、布局模型与事件流完全对齐 Android 原生开发概念（`Activity`、`View`、`ViewGroup`、`MotionEvent`、`LayoutParams` 等），实现零门槛上手。**

---

## 1. 业务需求与设计目标

### 1.1 业务背景与痛点

1. **跨语言开销**：传统 JVM UI（Android View / Jetpack Compose）或基于 JNI 的 Native 方案存在高频的跨语言调用与 GC 停顿开销。
2. **胶水层冗余**：通用跨平台胶水层（如 `android-activity`）抽象层级较重，隐藏了底层的 `ANativeActivityCallbacks` 与 `ALooper`，增加了控制灵活性损失与体积开销。

### 1.2 核心设计目标

1. **零胶水层与零 JNI 开销**：仅依赖 `raw-ndk-sys` 与 `raw-window-handle`，直接导出 C-ABI 函数 `ANativeActivity_onCreate`，通过 `ALooper` 手动轮询 `AInputQueue`，实现纯粹的底层 Native 响应。
2. **Android 概念 1:1 对齐**：所有的结构体、方法名、视图节点与事件名完全映射 Android 原生习惯（如 `Activity`、`View`、`ViewGroup`、`TextView`、`MotionEvent`、`LayoutParams` 等）。
3. **响应式单向数据流**：强约束的 Elm 架构（`Activity::on_create` -> `update` -> `on_draw`），保证 UI 与状态强一致性。
4. **Rust 2024 Edition 规范**：全代码库采用 Rust 2024 标准，提供高标准的类型安全与裸指针解引用界定。

---

## 2. 核心架构与 Android 概念映射

### 2.1 概念映射体系 (Concept Mapping)

| Android 原生概念      | Velm 对应的 Rust 结构体 / 枚举 | 职责与概念描述                                                                     |
| --------------------- | ------------------------------ | ---------------------------------------------------------------------------------- |
| **`Activity`**        | `trait Activity`               | 应用页面/生命周期容器（包含 `on_create`, `update`, `on_draw`, `on_touch_event`）。 |
| **`View`**            | `enum View<Msg>`               | 所有 UI 节点的统一抽象，包含基础样式与链式事件方法。                               |
| **`ViewGroup`**       | `struct ViewGroup<Msg>`        | 容器节点，持有 `children: Vec<View<Msg>>` 与布局方向。                             |
| **`TextView`**        | `struct TextView<Msg>`         | 文本绘制节点，支持 `text`, `text_size`, `text_color` 配置。                        |
| **`MotionEvent`**     | `struct MotionEvent`           | 触摸事件包装类，对齐 Android `ACTION_DOWN` / `MOVE` / `UP` 语义。                  |
| **`LayoutParams`**    | `struct LayoutParams`          | 布局参数，支持 `MATCH_PARENT`, `WRAP_CONTENT`, `Dp(f32)` 等。                      |
| **`OnClickListener`** | `.set_on_click_listener(msg)`  | 视图点击事件监听器绑定接口。                                                       |

### 2.2 系统架构拓扑图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       1. Velm Application (Activity)                        │
│   (State/Model) ──► (on_draw: State -> View) ──► (update: Message -> State) │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │ View Tree (ViewGroup / TextView)
┌──────────────────────────────────────▼──────────────────────────────────────┐
│                    2. Pure NDK Event Loop (ActivityThread)                  │
│  ┌─────────────────────────┐               ┌─────────────────────────────┐  │
│  │ ALooper_pollOnce        │──────────────►│ AInputQueue_getEvent        │  │
│  └────────────┬────────────┘               └──────────────┬──────────────┘  │
│               │ Window / Draw Events                      │ MotionEvent     │
│  ┌────────────▼────────────┐               ┌──────────────▼──────────────┐  │
│  │ ANativeWindow_setBuffers│               │ Dispatch to Activity        │  │
│  └────────────┬────────────┘               └──────────────┬──────────────┘  │
└───────────────┼───────────────────────────────────────────┼─────────────────┘
                │ Raw Window Pointer                        │ Pure Rust Event
┌───────────────▼───────────────────────────────────────────▼─────────────────┘
│            3. NativeWindowWrapper (raw-window-handle 0.5)                   │
│               wgpu::Surface / Vello Render Target                           │
└─────────────────────────────────────────────────────────────────────────────┘

```

---

## 3. 项目配置文件 (`Cargo.toml`)

采用 Rust `2024` Edition，严格限定底层依赖。

```toml
[package]
name = "velm-demo"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
# 严格限定的底层 NDK 与 Window 句柄依赖（零胶水层）
raw-ndk-sys = "0.1.2"
raw-window-handle = "0.5.2"

# 2D 矢量 GPU 渲染管线
vello = "0.2"
peniko = "0.1"

# 日志与调试
log = "0.4"
android_logger = "0.13"

# 异步任务处理
pin-project = "1.1"

```

---

## 4. 详细模块代码实现

### 4.1 平台底层句柄 (`platform::NativeWindowWrapper`)

利用 `raw-ndk-sys` 包装底层 `ANativeWindow` 指针，导出符合 `raw-window-handle v0.5` 规范的句柄。

```rust
//! platform/window.rs - 原始 NDK ANativeWindow 封装与 raw-window-handle 绑定

use std::ffi::c_void;
use std::ptr::NonNull;

use log::{error, info, trace};
use raw_ndk_sys::{
    ANativeWindow, ANativeWindow_getFormat, ANativeWindow_getHeight, ANativeWindow_getWidth,
    ANativeWindow_setBuffersGeometry, WINDOW_FORMAT_RGBA_8888,
};
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, HasRawDisplayHandle, HasRawWindowHandle,
    RawDisplayHandle, RawWindowHandle,
};

/// 安全包装底层 `ANativeWindow` 原始 C 指针。
/// 负责实现 `HasRawWindowHandle`，桥接 NDK 平台窗口与 WGPU/Vello 渲染管线。
pub struct NativeWindowWrapper {
    window_ptr: NonNull<ANativeWindow>,
}

impl NativeWindowWrapper {
    /// 从 NDK 原始指针安全创建包装对象。
    ///
    /// # Safety
    /// - 传入的 `ptr` 必须是有效且非空的 `ANativeWindow` 指针。
    pub unsafe fn from_raw_ndk_ptr(ptr: *mut ANativeWindow) -> Option<Self> {
        info!("[NativeWindowWrapper] Validating raw ANativeWindow pointer: {:p}", ptr);
        let window_ptr = NonNull::new(ptr)?;
        info!("[NativeWindowWrapper] Pointer validation passed.");
        Some(Self { window_ptr })
    }

    /// 获取 ANativeWindow 原始指针
    pub fn as_raw_ptr(&self) -> *mut ANativeWindow {
        self.window_ptr.as_ptr()
    }

    /// 显式配置 Native Window 的缓冲区尺寸与 RGBA_8888 颜色格式
    pub fn configure_buffers(&self, width: i32, height: i32) -> Result<(), i32> {
        let raw_ptr = self.as_raw_ptr();
        info!(
            "[NativeWindowWrapper] Setting geometry -> Width: {}px, Height: {}px, Format: RGBA_8888",
            width, height
        );

        unsafe {
            let result = ANativeWindow_setBuffersGeometry(
                raw_ptr,
                width,
                height,
                WINDOW_FORMAT_RGBA_8888 as i32,
            );

            if result == 0 {
                info!("[NativeWindowWrapper] Window buffers configured successfully.");
                Ok(())
            } else {
                error!("[NativeWindowWrapper] Buffer geometry config failed. Code: {}", result);
                Err(result)
            }
        }
    }

    /// 记录底层 Native Window 属性日志
    pub fn log_attributes(&self) {
        let raw_ptr = self.as_raw_ptr();
        unsafe {
            let width = ANativeWindow_getWidth(raw_ptr);
            let height = ANativeWindow_getHeight(raw_ptr);
            let format = ANativeWindow_getFormat(raw_ptr);
            info!(
                "[NativeWindowWrapper] Stats -> Width: {}px, Height: {}px, Format ID: {}",
                width, height, format
            );
        }
    }
}

impl HasRawWindowHandle for NativeWindowWrapper {
    fn raw_window_handle(&self) -> RawWindowHandle {
        trace!("[NativeWindowWrapper] Exporting RawWindowHandle::AndroidNdk...");
        let mut handle = AndroidNdkWindowHandle::empty();
        handle.a_native_window = self.window_ptr.as_ptr() as *mut c_void;
        RawWindowHandle::AndroidNdk(handle)
    }
}

impl HasRawDisplayHandle for NativeWindowWrapper {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        trace!("[NativeWindowWrapper] Exporting RawDisplayHandle::Android.");
        RawDisplayHandle::Android(AndroidDisplayHandle::empty())
    }
}

```

---

### 4.2 UI 视图与布局抽象 (`view::*`)

完全采用 Android 开发者熟悉的 `View`、`ViewGroup`、`TextView` 命名规范与链式调用模式。

```rust
//! view/mod.rs - Android 风格 UI 视图结构定义

use peniko::Color;

/// 尺寸定义规范（对齐 Android LayoutParams 尺寸常量）
#[derive(Clone, Copy, Debug)]
pub enum LayoutDimension {
    MatchParent,
    WrapContent,
    Dp(f32),
}

/// 对齐 Android ViewGroup.LayoutParams
#[derive(Clone, Debug)]
pub struct LayoutParams {
    pub width: LayoutDimension,
    pub height: LayoutDimension,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            width: LayoutDimension::WrapContent,
            height: LayoutDimension::WrapContent,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        }
    }
}

/// LinearLayout 布局方向
#[derive(Clone, Copy, Debug)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Android TextView 节点
pub struct TextView<Msg> {
    pub text: String,
    pub text_size: f32,
    pub text_color: Color,
    pub layout_params: LayoutParams,
    pub on_click_listener: Option<Msg>,
}

/// Android ViewGroup 容器节点
pub struct ViewGroup<Msg> {
    pub orientation: Orientation,
    pub layout_params: LayoutParams,
    pub children: Vec<View<Msg>>,
    pub on_click_listener: Option<Msg>,
}

/// 统一 View 节点抽象枚举 (对应 android.view.View)
pub enum View<Msg> {
    TextView(TextView<Msg>),
    ViewGroup(ViewGroup<Msg>),
}

impl<Msg: Clone> View<Msg> {
    /// 构造 LinearLayout 容器
    pub fn linear_layout(orientation: Orientation, children: Vec<View<Msg>>) -> Self {
        View::ViewGroup(ViewGroup {
            orientation,
            layout_params: LayoutParams {
                width: LayoutDimension::MatchParent,
                height: LayoutDimension::MatchParent,
                ..Default::default()
            },
            children,
            on_click_listener: None,
        })
    }

    /// 构造 TextView 节点
    pub fn text_view(text: impl Into<String>) -> Self {
        View::TextView(TextView {
            text: text.into(),
            text_size: 16.0,
            text_color: Color::WHITE,
            layout_params: LayoutParams::default(),
            on_click_listener: None,
        })
    }

    /// 链式设置文本字号
    pub fn set_text_size(mut self, size: f32) -> Self {
        if let View::TextView(ref mut tv) = self {
            tv.text_size = size;
        }
        self
    }

    /// 链式设置文本颜色
    pub fn set_text_color(mut self, color: Color) -> Self {
        if let View::TextView(ref mut tv) = self {
            tv.text_color = color;
        }
        self
    }

    /// 绑定 OnClickListener 监听器
    pub fn set_on_click_listener(mut self, msg: Msg) -> Self {
        match self {
            View::TextView(ref mut tv) => tv.on_click_listener = Some(msg),
            View::ViewGroup(ref mut vg) => vg.on_click_listener = Some(msg),
        }
        self
    }
}

```

---

### 4.3 触控事件解析 (`event::MotionEvent`)

对齐 `android.view.MotionEvent` API，直接从 NDK `AInputEvent` C 指针解析触控数据。

```rust
//! event/motion_event.rs - Android 风格触控事件

use raw_ndk_sys::{
    AInputEvent, AMOTION_EVENT_ACTION_CANCEL, AMOTION_EVENT_ACTION_DOWN, AMOTION_EVENT_ACTION_MOVE,
    AMOTION_EVENT_ACTION_UP, AMotionEvent_getAction, AMotionEvent_getX, AMotionEvent_getY,
    AINPUT_EVENT_TYPE_MOTION,
};

/// 对齐 Android MotionEvent.ACTION_*
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchAction {
    ActionDown,
    ActionMove,
    ActionUp,
    ActionCancel,
}

/// 对齐 android.view.MotionEvent
#[derive(Clone, Debug)]
pub struct MotionEvent {
    pub action: TouchAction,
    pub x: f32,
    pub y: f32,
}

impl MotionEvent {
    /// 从 NDK AInputEvent 原始 C 指针转换解析为 MotionEvent
    pub unsafe fn from_ndk_input_event(event: *const AInputEvent) -> Option<Self> {
        let event_type = unsafe { raw_ndk_sys::AInputEvent_getType(event) };
        if event_type != AINPUT_EVENT_TYPE_MOTION as i32 {
            return None;
        }

        let raw_action = unsafe { AMotionEvent_getAction(event) } & 0xff;
        let action = match raw_action as u32 {
            AMOTION_EVENT_ACTION_DOWN => TouchAction::ActionDown,
            AMOTION_EVENT_ACTION_MOVE => TouchAction::ActionMove,
            AMOTION_EVENT_ACTION_UP => TouchAction::ActionUp,
            AMOTION_EVENT_ACTION_CANCEL => TouchAction::ActionCancel,
            _ => return None,
        };

        let x = unsafe { AMotionEvent_getX(event, 0) };
        let y = unsafe { AMotionEvent_getY(event, 0) };

        Some(Self { action, x, y })
    }

    pub fn get_action(&self) -> TouchAction {
        self.action
    }

    pub fn get_x(&self) -> f32 {
        self.x
    }

    pub fn get_y(&self) -> f32 {
        self.y
    }
}

```

---

### 4.4 应用程序契约 (`app::Activity`)

生命周期与状态回调契约，沿用 Android `Activity` 概念。

```rust
//! app/activity.rs - Activity 声明与 Intent 抽象

use crate::event::motion_event::MotionEvent;
use crate::view::View;

/// 异步 Side-Effect 任务（对应 Elm Command，沿用 Android Intent 概念）
pub struct Intent<Message> {
    pub(crate) tasks: Vec<Box<pin_project::UnpinFuture<Message>>>,
}

impl<Message: 'static> Intent<Message> {
    pub fn none() -> Self {
        Self { tasks: Vec::new() }
    }
}

/// 核心 Activity 生命周期 Trait
pub trait Activity: Sized + 'static {
    type Message: Send + Clone + 'static;
    type SavedInstanceState;

    /// 页面创建入口（对应 onCreate）
    fn on_create(saved_state: Option<Self::SavedInstanceState>) -> (Self, Intent<Self::Message>);

    /// 状态更新逻辑 (TEA Update)
    fn update(&mut self, message: Self::Message) -> Intent<Self::Message>;

    /// 视图构建接口（对应 onDraw / setContentView）
    fn on_draw(&self) -> View<Self::Message>;

    /// 触控拦截事件（对应 onTouchEvent）
    fn on_touch_event(&mut self, _event: &MotionEvent) -> Option<Self::Message> {
        None
    }
}

```

---

### 4.5 零胶水层入口与事件驱动 (`engine::ActivityThread`)

直接接管 `ANativeActivityCallbacks` C 回调，并通过 `ALooper_pollOnce` 驱动事件循环，遵循 Rust 2024 FFI 规范。

```rust
//! engine/activity_thread.rs - 零胶水层的 ActivityThread 主事件循环调度器

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;

use log::{error, info, warn};
use raw_ndk_sys::{
    AInputQueue, AInputQueue_finishEvent, AInputQueue_getEvent, AInputQueue_preDispatchEvent,
    ALooper_pollOnce, ANativeActivity, ANativeWindow, ANativeWindow_getHeight,
    ANativeWindow_getWidth,
};

use crate::app::Activity;
use crate::event::motion_event::MotionEvent;
use crate::platform::NativeWindowWrapper;

/// 存储在 ANativeActivity::instance 中的内部状态句柄
struct AppContext<A: Activity> {
    activity_instance: A,
    message_queue: VecDeque<A::Message>,
    window_wrapper: Option<NativeWindowWrapper>,
    input_queue: *mut AInputQueue,
    is_running: bool,
}

/// 启动 Native Activity 并绑定底层 C 回调
pub unsafe fn run_native_activity<A: Activity>(
    activity: *mut ANativeActivity,
    _saved_state: *mut c_void,
    _saved_state_size: usize,
) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Trace)
            .with_tag("VelmEngine"),
    );

    info!("[ActivityThread] Executing ANativeActivity_onCreate...");

    let (activity_instance, _intent) = A::on_create(None);

    let context = Box::new(AppContext {
        activity_instance,
        message_queue: VecDeque::new(),
        window_wrapper: None,
        input_queue: ptr::null_mut(),
        is_running: true,
    });

    let context_ptr = Box::into_raw(context) as *mut c_void;
    
    unsafe {
        (*activity).instance = context_ptr;
        let callbacks = (*activity).callbacks;

        // 绑定 NativeActivity 回调函数指针表
        (*callbacks).onNativeWindowCreated = Some(on_native_window_created::<A>);
        (*callbacks).onNativeWindowDestroyed = Some(on_native_window_destroyed::<A>);
        (*callbacks).onInputQueueCreated = Some(on_input_queue_created::<A>);
        (*callbacks).onInputQueueDestroyed = Some(on_input_queue_destroyed::<A>);
        (*callbacks).onDestroy = Some(on_destroy::<A>);
    }

    info!("[ActivityThread] NativeActivity callbacks successfully bound.");
}

// ==================== C ABI Callbacks 绑定 ====================

unsafe extern "C" fn on_native_window_created<A: Activity>(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    info!("[ActivityThread] Callback: onNativeWindowCreated");
    let ctx = unsafe { &mut *((*activity).instance as *mut AppContext<A>) };

    if let Some(wrapper) = unsafe { NativeWindowWrapper::from_raw_ndk_ptr(window) } {
        let width = unsafe { ANativeWindow_getWidth(window) };
        let height = unsafe { ANativeWindow_getHeight(window) };
        let _ = wrapper.configure_buffers(width, height);
        ctx.window_wrapper = Some(wrapper);

        // 触发视图重绘
        let _root_view = ctx.activity_instance.on_draw();
    }
}

unsafe extern "C" fn on_native_window_destroyed<A: Activity>(
    activity: *mut ANativeActivity,
    _window: *mut ANativeWindow,
) {
    info!("[ActivityThread] Callback: onNativeWindowDestroyed");
    let ctx = unsafe { &mut *((*activity).instance as *mut AppContext<A>) };
    ctx.window_wrapper = None;
}

unsafe extern "C" fn on_input_queue_created<A: Activity>(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    info!("[ActivityThread] Callback: onInputQueueCreated");
    let ctx = unsafe { &mut *((*activity).instance as *mut AppContext<A>) };
    ctx.input_queue = queue;

    // 开启事件循环
    unsafe { drive_event_loop(ctx) };
}

unsafe extern "C" fn on_input_queue_destroyed<A: Activity>(
    activity: *mut ANativeActivity,
    _queue: *mut AInputQueue,
) {
    info!("[ActivityThread] Callback: onInputQueueDestroyed");
    let ctx = unsafe { &mut *((*activity).instance as *mut AppContext<A>) };
    ctx.input_queue = ptr::null_mut();
}

unsafe extern "C" fn on_destroy<A: Activity>(activity: *mut ANativeActivity) {
    info!("[ActivityThread] Callback: onDestroy");
    let context_ptr = unsafe { (*activity).instance as *mut AppContext<A> };
    if !context_ptr.is_null() {
        let mut ctx = unsafe { Box::from_raw(context_ptr) };
        ctx.is_running = false;
    }
}

/// 纯粹的 ALooper 事件循环（替代 android-activity 的事件轮询机制）
unsafe fn drive_event_loop<A: Activity>(ctx: &mut AppContext<A>) {
    info!("[ActivityThread] Entering ALooper event loop...");

    while ctx.is_running && !ctx.input_queue.is_null() {
        // 1. 从 AInputQueue 弹出输入事件
        let mut event = ptr::null_mut();
        while unsafe { AInputQueue_getEvent(ctx.input_queue, &mut event) } >= 0 {
            if unsafe { AInputQueue_preDispatchEvent(ctx.input_queue, event) } == 0 {
                if let Some(motion_event) = unsafe { MotionEvent::from_ndk_input_event(event) } {
                    if let Some(msg) = ctx.activity_instance.on_touch_event(&motion_event) {
                        ctx.message_queue.push_back(msg);
                    }
                }
                unsafe { AInputQueue_finishEvent(ctx.input_queue, event, 1) };
            }
        }

        // 2. 消费 Message 队列并触发 update
        let mut needs_draw = false;
        while let Some(msg) = ctx.message_queue.pop_front() {
            let _intent = ctx.activity_instance.update(msg);
            needs_draw = true;
        }

        // 3. 驱动重绘流程
        if needs_draw && ctx.window_wrapper.is_some() {
            let _root_view = ctx.activity_instance.on_draw();
        }

        // 4. 挂起等待系统 Looper 唤醒（16ms 挂起以防 CPU 满载）
        unsafe { ALooper_pollOnce(16, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) };
    }
}

```

---

## 5. 开发者视角与入口导出 (`lib.rs`)

开发者编写标准的 `MainActivity` 代码，并导出 C-ABI 函数 `ANativeActivity_onCreate`：

```rust
//! lib.rs - 开发者编写的入口文件

use peniko::Color;
use velm_demo::app::{Activity, Intent};
use velm_demo::event::motion_event::MotionEvent;
use velm_demo::view::{Orientation, View};

pub struct MainActivity {
    counter: i32,
}

#[derive(Clone, Debug)]
pub enum MainMessage {
    ClickIncrement,
    ClickDecrement,
}

impl Activity for MainActivity {
    type Message = MainMessage;
    type SavedInstanceState = ();

    fn on_create(_saved_state: Option<()>) -> (Self, Intent<Self::Message>) {
        (MainActivity { counter: 0 }, Intent::none())
    }

    fn update(&mut self, message: Self::Message) -> Intent<Self::Message> {
        match message {
            MainMessage::ClickIncrement => self.counter += 1,
            MainMessage::ClickDecrement => self.counter -= 1,
        }
        Intent::none()
    }

    fn on_draw(&self) -> View<Self::Message> {
        View::linear_layout(
            Orientation::Vertical,
            vec![
                View::text_view(format!("Current Count: {}", self.counter))
                    .set_text_size(28.0)
                    .set_text_color(Color::WHITE),
                View::text_view("Button: +1")
                    .set_text_size(20.0)
                    .set_text_color(Color::GREEN)
                    .set_on_click_listener(MainMessage::ClickIncrement),
                View::text_view("Button: -1")
                    .set_text_size(20.0)
                    .set_text_color(Color::RED)
                    .set_on_click_listener(MainMessage::ClickDecrement),
            ],
        )
    }

    fn on_touch_event(&mut self, _event: &MotionEvent) -> Option<Self::Message> {
        None
    }
}

/// 导出系统识别的 ANativeActivity_onCreate 入口（Rust 2024 Edition 规范）
#[no_mangle]
pub unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut raw_ndk_sys::ANativeActivity,
    saved_state: *mut std::ffi::c_void,
    saved_state_size: usize,
) {
    assert!(!activity.is_null(), "[Velm] Received NULL ANativeActivity pointer!");

    unsafe {
        velm_demo::engine::run_native_activity::<MainActivity>(
            activity,
            saved_state,
            saved_state_size,
        );
    }
}

```