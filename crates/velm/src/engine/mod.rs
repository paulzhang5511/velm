//! 引擎层：NativeActivity 回调绑定、引擎线程与事件循环。
//!
//! 仅在 Android 目标编译（FFI 边界，SPEC §7.7）；host 上不存在本模块，
//! 以保证框架纯逻辑可在主机测试。

#[cfg(target_os = "android")]
pub mod activity_thread;
