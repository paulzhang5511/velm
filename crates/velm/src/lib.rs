//! # Velm
//!
//! 基于 vello GPU 渲染与 Elm（TEA）架构的 Android 零胶水 NativeActivity
//! 2D GUI 框架。
//!
//! 模块随实现进度开放（见 docs/PLAN.md T1~T16）：
//!
//! - `app`：[`Activity`] trait 与运行时状态（T8）
//! - `view`：View/ViewGroup/TextView 与链式构造器（T4）
//! - `layout`：手写 LinearLayout 测量与布局（T6）
//! - `event`：MotionEvent 与触摸动作映射（T5）
//! - `engine`：NativeActivity 回调、引擎线程、命中测试（T3/T7/T8/T12）
//! - `platform`：ANativeWindow 封装与 raw-window-handle（T9）
//! - `render`：绘制指令与 vello/wgpu 渲染器（T2/T10）
//! - `error`：框架统一错误类型（ADR-11）

// 公共 re-export（SPEC §7.9）：应用 crate 只需 `use velm::{...}`。
pub use app::{Activity, Intent};
pub use event::{MotionEvent, TouchAction};
/// 颜色类型：`on_draw` 构造背景 / 文字颜色时使用（`peniko::Color` 的再导出，
/// 省去应用 crate 再声明一次 peniko 依赖）。
pub use peniko::Color;
pub use platform::{DensityBucket, DisplayMetrics};
pub use view::{
    Background, CommonStyle, EdgeInsets, ImageScale, LayoutDimension, LayoutParams, Orientation,
    ProgressOrientation, Rect, Stroke, TextView, View, ViewGroup, WidgetKind, WidgetView,
};

/// 应用入口（android-only：依赖 NDK 回调表）。
#[cfg(target_os = "android")]
pub use engine::run_native_activity;

pub mod app;
pub mod engine;
pub mod error;
pub mod event;
pub mod layout;
pub mod platform;
pub mod render;
pub mod view;
