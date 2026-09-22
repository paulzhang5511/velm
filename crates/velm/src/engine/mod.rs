//! 引擎层：命中测试 + NativeActivity 回调与事件循环。
//!
//! 本模块**在 host 可见**，但只有其中的纯逻辑部分会真正编译进来：
//!
//! - `hit_test`：纯 Rust、host 可直接单测（SPEC §10.2 硬约束、§7.5）；
//! - `activity_thread`：处于 FFI 边界，仅在 Android 目标编译，host 上不存在。
//!
//! 因此 `engine` 模块本身不能在 lib.rs 里整块 `#[cfg(target_os = "android")]`
//! 关掉，否则命中测试会失去 host 可测性——android-only 的部分在**子模块**
//! 上单独 gate。

pub mod hit_test;

#[cfg(target_os = "android")]
pub mod activity_thread;
