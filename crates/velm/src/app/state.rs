//! app/state.rs — TEA 运行时状态：Model + 消息队列 + 重绘标志。
//!
//! 把「消费消息 → update → 请求重绘」这条链路从引擎主循环里剥出来，使其
//! 成为 host 可测的纯逻辑（§10.2）；T12 的引擎循环只负责取事件、调本模块、
//! 再按标志决定是否渲染。
//!
//! 与 [`crate::engine::events::EngineState`] 的分工：后者管**窗口 / 队列**
//! 生命周期，本模块管**应用 Model 与消息**；两者都置重绘标志时才会真正出帧。

use std::collections::VecDeque;

use crate::event::MotionEvent;
use crate::view::View;

use super::activity::Activity;

/// Activity 运行时：持有 Model、待处理消息队列与重绘标志。
pub struct ActivityRuntime<A: Activity> {
    activity: A,
    messages: VecDeque<A::Message>,
    needs_draw: bool,
}

impl<A: Activity> ActivityRuntime<A> {
    /// 以初始 Model 构造（重绘标志初始为 false：首帧由窗口创建事件触发）。
    pub fn new(activity: A) -> Self {
        Self {
            activity,
            messages: VecDeque::new(),
            needs_draw: false,
        }
    }

    /// 从 [`Activity::on_create`] 构造（v1 传入 `None`）。
    pub fn create() -> Self {
        let (activity, intent) = A::on_create(None);
        log::trace!("[App] on_create 返回 Intent（v1 不执行）: {intent:?}");
        Self::new(activity)
    }

    /// 只读访问 Model。
    pub fn activity(&self) -> &A {
        &self.activity
    }

    /// 可变访问 Model（引擎分发触控拦截等场景）。
    pub fn activity_mut(&mut self) -> &mut A {
        &mut self.activity
    }

    /// 待处理消息条数。
    pub fn pending(&self) -> usize {
        self.messages.len()
    }

    /// 入队一条消息（不立即 update：统一在 [`Self::drain`] 中处理，
    /// 保证「一帧内只因消息重建一次视图树」，§7.7）。
    pub fn enqueue(&mut self, message: A::Message) {
        self.messages.push_back(message);
    }

    /// 是否请求了重绘。
    pub fn needs_draw(&self) -> bool {
        self.needs_draw
    }

    /// 取走重绘标志（取走后清零）。
    pub fn take_draw_request(&mut self) -> bool {
        let requested = self.needs_draw;
        self.needs_draw = false;
        requested
    }

    /// 消费全部待处理消息：逐条 `update`，任一消息被处理即置重绘标志。
    ///
    /// 返回本次处理的条数。`update` 返回的 Intent 在 v1 只记录不执行（§7.6）。
    pub fn drain(&mut self) -> usize {
        let mut handled = 0;
        while let Some(message) = self.messages.pop_front() {
            let intent = self.activity.update(message);
            log::trace!("[App] update 返回 Intent（v1 不执行）: {intent:?}");
            handled += 1;
            self.needs_draw = true;
        }
        handled
    }

    /// 触控拦截：`Activity::on_touch_event` 返回 `Some` 时入队并返回 `true`，
    /// 引擎据此**跳过**默认 hit-test（FR-I2）。
    pub fn on_touch_event(&mut self, event: &MotionEvent) -> bool {
        match self.activity.on_touch_event(event) {
            Some(message) => {
                self.enqueue(message);
                true
            }
            None => false,
        }
    }

    /// 取当前帧视图树（`on_draw`）。
    ///
    /// 引擎每帧最多调用一次：docs 在 ActionDown 分支与 update 后各调一次，
    /// 一触可能重建两棵树，属规格禁止项（§7.7）。
    pub fn view(&self) -> View<A::Message> {
        self.activity.on_draw()
    }
}
