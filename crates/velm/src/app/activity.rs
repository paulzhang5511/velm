//! app/activity.rs — Activity / Intent 契约（SPEC §7.6）。
//!
//! v1 的 `Intent` 是**纯占位**：保留类型与 API 形状，`on_create` / `update`
//! 返回的 Intent 被引擎记录（trace）但不执行，不得因此报错。P1 再接入任务
//! 执行器与 SavedInstanceState 存取。

use std::fmt;
use std::marker::PhantomData;

use crate::event::MotionEvent;
use crate::view::View;

/// v1 占位 Intent：不携带任何可执行任务。
///
/// `Message` 只作为类型参数出现（用于约束 API 形状），不参与运行时。
pub struct Intent<Message> {
    _message: PhantomData<Message>,
}

impl<Message> Intent<Message> {
    /// 构造一个「无动作」的 Intent。
    pub fn none() -> Self {
        Self {
            _message: PhantomData,
        }
    }
}

// 手写实现而非 derive：derive 会为 `Message` 加上 `Clone/Debug/Default` 约束，
// 而 Intent 并不实际持有 Message（PhantomData），不该给使用者加负担。
impl<Message> Clone for Intent<Message> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Message> Copy for Intent<Message> {}

impl<Message> Default for Intent<Message> {
    fn default() -> Self {
        Self::none()
    }
}

impl<Message> fmt::Debug for Intent<Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Intent::none()")
    }
}

/// TEA 风格的 Activity 契约（Elm 架构：Model = Self，Msg = Self::Message）。
pub trait Activity: Sized + 'static {
    /// 消息类型；`Send` 是引擎线程 / channel 的要求（§3.3）。
    type Message: Send + Clone + 'static;

    /// 保存的实例状态类型；v1 恒为 `()`，不参与序列化。
    type SavedInstanceState;

    /// 创建 Activity 与首个 Intent。v1 中 `saved` 恒为 `None`。
    fn on_create(saved: Option<Self::SavedInstanceState>) -> (Self, Intent<Self::Message>);

    /// 处理一条消息，返回下一个 Intent（v1 不执行）。
    fn update(&mut self, message: Self::Message) -> Intent<Self::Message>;

    /// 渲染：返回当前 Model 对应的视图树（每帧重建，随帧释放）。
    fn on_draw(&self) -> View<Self::Message>;

    /// 开发者自定义触控拦截；返回 `Some(msg)` 时消息入队，且**不再**走默认 hit-test。
    fn on_touch_event(&mut self, _event: &MotionEvent) -> Option<Self::Message> {
        None
    }
}
