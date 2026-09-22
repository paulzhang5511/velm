//! app/ — 开发者面向的应用层契约与运行时状态（SPEC §7.6）。
//!
//! - `activity`：`Activity` trait 与占位 `Intent`（TEA：Model = Self，Msg = Self::Message）；
//! - `state`：`ActivityRuntime`，把「消费消息 → update → 请求重绘」从引擎循环
//!   中剥离为 host 可测的纯逻辑（§10.2）。
//!
//! 窗口 / 队列生命周期状态机在 [`crate::engine::events`]，本模块不涉及。

pub mod state;

mod activity;

pub use activity::{Activity, Intent};
