//! engine/events.rs — 引擎输入事件、动作与生命周期状态机（SPEC §3.4 / §7.7）。
//!
//! 本模块是**纯 host 逻辑**：不依赖 Looper、NDK 或 wgpu，可在开发机直接单测。
//! T12 的引擎主循环只需把 C 回调翻译成 [`EngineEvent`]、执行返回的
//! [`EngineAction`]；「窗口与队列到达顺序任意」「无窗口时丢弃消息 / 重绘」
//! 「Quit 后不再产生动作」等不变量全部收敛在这里，并有单测钉住（§10.2）。
//!
//! 状态机刻意**不持有** ANativeWindow / AInputQueue 指针：指针生命周期由
//! `activity_thread` 负责，本模块只跟踪「有没有」与「尺寸是多少」。

/// 窗口视口：物理像素宽高 + density（ADR-12，dpi/160）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// 窗口宽度（物理像素）。
    pub width: i32,
    /// 窗口高度（物理像素）。
    pub height: i32,
    /// 密度（dpi/160）；拿不到时兜底 1.0。
    pub density: f32,
}

/// 引擎线程的输入事件。
///
/// `Message` / `Redraw` 只表达「到了一条」，消息值本身由引擎的消息队列持有——
/// 状态机不关心内容，只决定它该被处理还是被丢弃。
// 只派生 PartialEq：`Viewport` 含 f32（density），不满足 Eq。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineEvent {
    /// 窗口创建（主线程已 acquire，所有权移交引擎）。
    WindowCreated(Viewport),
    /// 同一窗口尺寸变化（旋转 / 分屏）：指针与所有权不变，density 沿用旧值。
    WindowResized {
        /// 新的宽度（物理像素）。
        width: i32,
        /// 新的高度（物理像素）。
        height: i32,
    },
    /// 窗口销毁：引擎必须停止 present 并 release。
    WindowDestroyed,
    /// 输入队列创建。
    QueueCreated,
    /// 输入队列销毁。
    QueueDestroyed,
    /// 一条 Message 到达（来自 hit-test 或 `on_touch_event` 拦截）。
    Message,
    /// 外部请求重绘（首帧、配置变更等）。
    Redraw,
    /// Activity 销毁：退出引擎线程。
    Quit,
}

/// 状态机产出的动作，由引擎主循环按序执行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineAction {
    /// 创建 surface 与渲染器（进入 HasSurface）。
    CreateSurface,
    /// 按新尺寸重配 surface（FR-L3）。
    ResizeSurface,
    /// 丢弃 surface 与渲染器；此后禁止再 present。
    DestroySurface,
    /// 把输入队列 attach 到引擎线程 Looper。
    AttachQueue,
    /// 从 Looper detach 输入队列。
    DetachQueue,
    /// 退出引擎主循环。
    Exit,
}

/// 引擎生命周期状态（SPEC §3.4）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineState {
    /// 当前窗口视口；`None` 表示无可用 surface。
    window: Option<Viewport>,
    /// 输入队列是否已 attach。
    queue: bool,
    /// 是否已收到 Quit。置位后 [`step`] 对任何事件都不再产生动作。
    quit: bool,
    /// 是否请求了重绘（取走即清零，见 [`EngineState::take_draw_request`]）。
    needs_draw: bool,
}

impl EngineState {
    /// 初始状态：无窗口、无队列、未退出。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前视口；`None` 表示无可用 surface（渲染必须跳过）。
    pub fn viewport(&self) -> Option<Viewport> {
        self.window
    }

    /// 是否持有可用 surface。
    pub fn has_window(&self) -> bool {
        self.window.is_some()
    }

    /// 输入队列是否已 attach。
    pub fn has_queue(&self) -> bool {
        self.queue
    }

    /// 是否已收到 Quit。
    pub fn is_quitting(&self) -> bool {
        self.quit
    }

    /// 是否请求了重绘。
    pub fn needs_draw(&self) -> bool {
        self.needs_draw
    }

    /// 取走重绘标志（取走后清零）：引擎据此决定本帧是否 `on_draw` + 渲染。
    pub fn take_draw_request(&mut self) -> bool {
        let requested = self.needs_draw;
        self.needs_draw = false;
        requested
    }
}

/// 状态机唯一入口：处理一个事件，返回引擎需要执行的动作序列。
///
/// 不变量（均有单测覆盖，见 `tests/state_machine.rs`）：
/// - 窗口与队列的到达顺序任意，两者独立跟踪；
/// - 无窗口时到达的 Message / Redraw 一律丢弃（warn）且不置重绘标志；
/// - 窗口销毁后清除未消费的重绘标志（禁止在无 surface 时渲染）；
/// - Quit 之后任何事件都不再产生动作。
pub fn step(state: &mut EngineState, event: EngineEvent) -> Vec<EngineAction> {
    if state.quit {
        log::warn!("[Engine] 已收到 Quit，忽略后续事件: {event:?}");
        return Vec::new();
    }

    let mut actions = Vec::new();
    match event {
        EngineEvent::WindowCreated(viewport) => {
            if state.window.is_some() {
                // 正常路径上不会发生（销毁回调必先到达）；兜底避免泄漏旧 surface。
                log::warn!("[Engine] 窗口重复创建，先销毁旧 surface 再重建");
                actions.push(EngineAction::DestroySurface);
            }
            state.window = Some(viewport);
            state.needs_draw = true; // 首帧
            actions.push(EngineAction::CreateSurface);
        }
        EngineEvent::WindowResized { width, height } => match state.window {
            None => log::warn!("[Engine] 无窗口时收到 resize，丢弃"),
            Some(viewport) => {
                // density 不随 resize 事件送达（T3 的回调只给宽高），沿用旧值。
                state.window = Some(Viewport {
                    width,
                    height,
                    density: viewport.density,
                });
                state.needs_draw = true;
                actions.push(EngineAction::ResizeSurface);
            }
        },
        EngineEvent::WindowDestroyed => {
            if state.window.take().is_none() {
                log::warn!("[Engine] 无窗口时收到销毁，忽略");
                return actions;
            }
            // 无 surface 后不得再渲染：丢弃尚未消费的重绘请求。
            state.needs_draw = false;
            actions.push(EngineAction::DestroySurface);
        }
        EngineEvent::QueueCreated => {
            if state.queue {
                log::warn!("[Engine] 输入队列重复创建，先 detach 旧队列");
                actions.push(EngineAction::DetachQueue);
            }
            state.queue = true;
            actions.push(EngineAction::AttachQueue);
        }
        EngineEvent::QueueDestroyed => {
            if !state.queue {
                log::warn!("[Engine] 无输入队列时收到销毁，忽略");
                return actions;
            }
            state.queue = false;
            actions.push(EngineAction::DetachQueue);
        }
        EngineEvent::Message | EngineEvent::Redraw => {
            if !state.has_window() {
                log::warn!("[Engine] 无可用窗口，丢弃事件: {event:?}");
                return actions;
            }
            state.needs_draw = true;
        }
        EngineEvent::Quit => {
            state.quit = true;
            state.needs_draw = false;
            // 退出前把仍持有的资源还回去：顺序为「先停渲染，再停输入」。
            if state.window.take().is_some() {
                actions.push(EngineAction::DestroySurface);
            }
            if state.queue {
                state.queue = false;
                actions.push(EngineAction::DetachQueue);
            }
            actions.push(EngineAction::Exit);
        }
    }

    actions
}
