//! # Velm
//!
//! 基于 vello GPU 渲染与 Elm（TEA）架构的 Android 零胶水 NativeActivity
//! 2D GUI 框架。
//!
//! 模块随实现进度开放（见 docs/PLAN.md T1~T16）：
//!
//! - `app`：[`Activity`] trait 与运行时状态（T8）
//! - `view`：View/ViewGroup/TextView 与链式构造器（T4）
//! - `layout`：手写 LinearLayout 测量与布局（T5）
//! - `event`：MotionEvent 与触摸动作映射（T6）
//! - `engine`：NativeActivity 回调、引擎线程、命中测试（T3/T7/T8/T12）
//! - `platform`：ANativeWindow 封装与 raw-window-handle（T9）
//! - `render`：vello/wgpu 渲染器（T2/T10）

#[cfg(target_os = "android")]
pub mod engine;
