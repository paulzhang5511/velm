//! event/ — 触控事件解码（SPEC §7.3）。
//!
//! `decode_action` 与 `MotionEvent` 为纯 host 逻辑；只有 `MotionEvent::from_ndk`
//! 接触 NDK 且仅在 android 目标编译（§10.2 硬约束）。

mod motion_event;

pub use motion_event::{MotionEvent, TouchAction, decode_action};
