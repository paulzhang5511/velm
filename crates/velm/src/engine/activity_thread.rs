//! NativeActivity 入口、C 回调绑定与引擎线程。
//!
//! 事件模型（SPEC §3.3 / ADR-09）：
//! - 主线程（UI）的 C 回调只做 acquire / 登记 / channel 发布，绝不阻塞渲染
//!   或轮询（唯一的例外是销毁路径的**同步 ack**：窗口 / 队列销毁回调必须阻塞
//!   到引擎线程停止使用该资源后才返回，否则 NDK 会 use-after-free）；
//! - 引擎线程持有自己的 [`ALooper`]，输入队列只在该线程 attach/detach；
//! - onDestroy 发 Quit、join 引擎线程并回收上下文。
//!
//! T12：引擎主循环改为由 §3.4 状态机驱动——控制消息先翻译成
//! [`EngineEvent`]、交给 [`step`] 得到 [`EngineAction`]，再由循环执行动作。
//! 「窗口与队列到达顺序任意」「无窗口丢弃重绘」「Quit 后不再出帧」等不变量
//! 全部由状态机与其单测保证，本文件只做翻译与执行。
//!
//! 输入事件仍只记录日志（T13 接入 hit-test 与消息分发）。

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::AtomicPtr;
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use raw_ndk_sys::{
    ACONFIGURATION_DENSITY_ANY, ACONFIGURATION_DENSITY_NONE, AConfiguration_delete,
    AConfiguration_fromAssetManager, AConfiguration_getDensity, AConfiguration_new, AInputQueue,
    AInputQueue_attachLooper, AInputQueue_detachLooper, AInputQueue_finishEvent,
    AInputQueue_getEvent, AInputQueue_preDispatchEvent, ALOOPER_POLL_ERROR,
    ALOOPER_PREPARE_ALLOW_NON_CALLBACKS, ALooper, ALooper_pollOnce, ALooper_prepare,
    ANativeActivity, ANativeActivityCallbacks, ANativeWindow, ANativeWindow_acquire,
    ANativeWindow_getHeight, ANativeWindow_getWidth, ANativeWindow_release,
};

use crate::app::Activity;
use crate::app::state::ActivityRuntime;
use crate::engine::app_context::{AppContext, EngineMsg, NdkPtr, null_looper_slot};
use crate::engine::events::{EngineAction, EngineEvent, EngineState, Viewport, step};
use crate::engine::hit_test::perform_hit_test;
use crate::event::{MotionEvent, TouchAction};
use crate::layout::measure_and_layout_with;
use crate::platform::DisplayMetrics;
use crate::platform::window::NativeWindowWrapper;
use crate::render::VelloRenderer;
use crate::view::View;

/// 日志 tag（logcat `-s VelmEngine`）。
const LOG_TAG: &str = "VelmEngine";
/// attachLooper 的 looper ident；pollOnce 返回该值表示输入队列有事件。
const INPUT_QUEUE_IDENT: i32 = 1;
/// pollOnce 超时兜底（毫秒，ADR-09）。
const POLL_TIMEOUT_MS: i32 = 16;

/// 引擎线程持有的资源（生命周期由 §3.4 状态机决定）。
struct Resources<M> {
    /// 当前持有的窗口引用（接手回调侧 acquire 的那一份）。
    window: Option<NativeWindowWrapper>,
    /// 渲染器；`Some` ⟺ 状态机处于 HasSurface。
    renderer: Option<VelloRenderer>,
    /// 已 attach 到本线程 Looper 的输入队列。
    queue: *mut AInputQueue,
    /// 待 attach 的队列指针（由 `QueueCreated` 消息带来，`AttachQueue` 取用）。
    pending_queue: *mut AInputQueue,
    /// 最近一次**已布局**的视图树：T13 的 hit-test 直接复用它，避免
    /// 「一触重建两次」（SPEC §7.7 性能约束）。
    frame: Option<View<M>>,
}

impl<M> Default for Resources<M> {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            queue: ptr::null_mut(),
            pending_queue: ptr::null_mut(),
            frame: None,
        }
    }
}

/// 框架入口：由 cdylib 导出的 `ANativeActivity_onCreate` 调用（SPEC §7.9）。
///
/// `A` 是应用实现的 [`Activity`]。这里在**主线程**完成 `A::on_create` 并把
/// 运行时一次性移入引擎线程，故要求 `A: Send`（Model 此后只在引擎线程被访问，
/// 绝大多数 Model 自动满足）。
///
/// # Safety
/// `activity` 必须是 Android 框架传入的有效、非空、在本次 onCreate
/// 调用期间存活的 [`ANativeActivity`] 指针。`saved_state` 可为空；
/// 非空时其前 `saved_state_size` 字节必须可读（v1 不恢复状态，仅记录）。
pub unsafe fn run_native_activity<A: Activity + Send>(
    activity: *mut ANativeActivity,
    saved_state: *mut c_void,
    saved_state_size: usize,
) {
    let runtime = ActivityRuntime::<A>::create();
    log::info!("Activity 已创建，运行时移入引擎线程");
    // SAFETY: 契约同 bootstrap。
    unsafe { bootstrap(activity, saved_state, saved_state_size, runtime) }
}

/// 启动框架：初始化 logger、绑定 C 回调、启动引擎线程。
///
/// # Safety
/// 同 [`run_native_activity`]。
unsafe fn bootstrap<A: Activity + Send>(
    activity: *mut ANativeActivity,
    saved_state: *mut c_void,
    saved_state_size: usize,
    runtime: ActivityRuntime<A>,
) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        android_logger::init_once(
            android_logger::Config::default()
                // 仅 Info：wgpu/naga/vello 的 Trace/Debug 日志（shader 编译）会
                // 冲爆 logcat ring buffer 并拖慢软件 Vulkan 启动；框架自身只用
                // info/warn/error。
                .with_max_level(log::LevelFilter::Info)
                .with_tag(LOG_TAG),
        );

        log::info!(
            "ANativeActivity_onCreate: saved_state_size={}",
            saved_state_size
        );
        if !saved_state.is_null() {
            // v1 不支持状态恢复（SPEC §7.6 SavedInstanceState = P1）。
            log::warn!("收到 saved_state（{} 字节），v1 将忽略", saved_state_size);
        }

        if activity.is_null() || unsafe { (*activity).callbacks }.is_null() {
            log::error!("activity 或 callbacks 为空，无法绑定回调");
            return;
        }

        let (tx, rx) = unbounded();
        let looper = null_looper_slot();

        let join = match thread::Builder::new().name("velm-engine".into()).spawn({
            let looper = looper.clone();
            move || engine_main::<A>(rx, looper, runtime)
        }) {
            Ok(j) => j,
            Err(e) => {
                log::error!("引擎线程启动失败: {e}");
                return;
            }
        };

        // 把上下文发布到 activity.instance（所有权移交，onDestroy 取回）。
        // SAFETY: 契约保证 activity 在 onCreate 期间有效独占。
        if !unsafe { AppContext::install(activity, tx, looper, join) } {
            log::error!("activity 为空，无法安装上下文");
        }

        // SAFETY: 同上，callbacks 在 Activity 生命周期内有效。
        let callbacks: &mut ANativeActivityCallbacks = unsafe { &mut *(*activity).callbacks };
        callbacks.onNativeWindowCreated = Some(native_window_created);
        callbacks.onNativeWindowResized = Some(native_window_resized);
        callbacks.onNativeWindowDestroyed = Some(native_window_destroyed);
        callbacks.onInputQueueCreated = Some(input_queue_created);
        callbacks.onInputQueueDestroyed = Some(input_queue_destroyed);
        callbacks.onDestroy = Some(on_destroy);

        log::info!("已绑定 6 个生命周期回调，引擎线程已启动");
    }));

    if result.is_err() {
        log::error!("bootstrap 发生 panic，已抑制以避免跨 FFI unwind");
    }
}

/// 引擎线程主函数：prepare Looper、attach 输入队列、轮询分发与出帧。
fn engine_main<A: Activity>(
    rx: Receiver<EngineMsg>,
    looper_slot: Arc<AtomicPtr<ALooper>>,
    mut runtime: ActivityRuntime<A>,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 本线程首次 prepare，返回与本线程绑定的 Looper；
        // ALLOW_NON_CALLBACKS 允许 pollOnce 直接返回 fd ident。
        let looper = unsafe { ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as i32) };
        if looper.is_null() {
            log::error!("ALooper_prepare 返回空，引擎线程退出");
            return;
        }
        AppContext::publish_looper(&looper_slot, looper);
        log::info!("引擎线程 Looper 就绪");

        let mut state = EngineState::new();
        let mut res = Resources::<A::Message>::default();

        'outer: loop {
            // 先排空控制通道，保证销毁/退出消息优先于输入处理。
            while let Ok(msg) = rx.try_recv() {
                // 同步 ack 必须在动作**执行完之后**才回（销毁回调正阻塞等待）。
                let ack = dispatch(msg, &mut state, &mut res, looper);
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
                if state.is_quitting() {
                    break 'outer;
                }
            }

            // 2) 轮询等待：输入事件在此被取出、解码并转成应用消息（T13）。
            // SAFETY: 本线程持有 looper；空指针出参表示不取 fd/events/data。
            let poll_rc = unsafe {
                ALooper_pollOnce(
                    POLL_TIMEOUT_MS,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };

            let queue = res.queue;
            if poll_rc == INPUT_QUEUE_IDENT && !queue.is_null() {
                drain_input(queue, &mut runtime, &mut state, &mut res);
            } else if poll_rc == ALOOPER_POLL_ERROR {
                log::error!("ALooper_pollOnce 返回 ERROR，引擎线程退出");
                break;
            }
            // WAKE(-1) / TIMEOUT(-3) / CALLBACK(-2，未用回调) 均回到顶部收消息。

            // 3) 消费消息：逐条 `update`，合并成**一次**重绘（§7.7）——
            //    不允许每条消息各重建一次视图树。
            let handled = runtime.drain();
            if handled > 0 {
                log::info!("[App] 本帧处理 {handled} 条消息");
            }

            // 4) 出帧：状态机置位（窗口创建 / resize / Message）或运行时置位。
            if state.take_draw_request() || runtime.needs_draw() {
                runtime.take_draw_request();
                draw_frame(
                    &mut runtime,
                    &mut res.renderer,
                    state.viewport(),
                    &mut res.frame,
                );
            }
        }
    }));

    if outcome.is_err() {
        log::error!("引擎线程发生 panic，受控退出");
    }
    log::info!("引擎线程已退出");
}

/// 把一条控制消息翻译成 §3.4 的 [`EngineEvent`]，交给状态机并执行返回的动作。
///
/// 返回需要回执的一次性 ack 通道：`WindowDestroyed` / `QueueDestroyed` 的
/// 回调正阻塞等待它，因此**必须**在动作执行完之后、由调用方立即回执——
/// 指针不匹配、状态机未产生动作等分支也不例外（否则主线程永久阻塞）。
fn dispatch<M>(
    msg: EngineMsg,
    state: &mut EngineState,
    res: &mut Resources<M>,
    looper: *mut ALooper,
) -> Option<Sender<()>> {
    match msg {
        EngineMsg::InputQueueCreated(NdkPtr(q)) => {
            res.pending_queue = q;
            run(state, EngineEvent::QueueCreated, res, looper);
            None
        }
        EngineMsg::InputQueueDestroyed(NdkPtr(q), ack) => {
            if res.queue != q {
                log::warn!("QueueDestroyed 与当前 attached 队列不一致");
            }
            run(state, EngineEvent::QueueDestroyed, res, looper);
            Some(ack)
        }
        EngineMsg::WindowCreated {
            window: NdkPtr(w),
            width,
            height,
            density,
        } => {
            // 接手回调中已 acquire 的引用（不二次 acquire）。
            // SAFETY: 回调侧已 acquire 且窗口在回调期间有效。
            let Some(owned) = (unsafe { NativeWindowWrapper::from_ndk_owned(w) }) else {
                log::error!("窗口指针为空，忽略 WindowCreated");
                return None;
            };
            if res.window.is_some() {
                log::warn!("新窗口到达时旧窗口仍持有，先销毁旧 surface 与引用");
            }
            log::info!("引擎获得窗口 {w:p}：{width}x{height} density={density:.2}");
            res.window = Some(owned);
            run(
                state,
                EngineEvent::WindowCreated(Viewport {
                    width,
                    height,
                    density,
                }),
                res,
                looper,
            );
            None
        }
        EngineMsg::WindowDestroyed(NdkPtr(w), ack) => {
            let matches = res
                .window
                .as_ref()
                .map(|held| held.as_raw_ptr() == w)
                .unwrap_or(false);
            if matches {
                run(state, EngineEvent::WindowDestroyed, res, looper);
            } else {
                log::warn!("WindowDestroyed 与当前持有窗口不一致");
            }
            Some(ack)
        }
        EngineMsg::WindowResized { width, height } => {
            run(
                state,
                EngineEvent::WindowResized { width, height },
                res,
                looper,
            );
            None
        }
        EngineMsg::Quit => {
            run(state, EngineEvent::Quit, res, looper);
            None
        }
    }
}

/// 状态机单步：取动作并按序执行。
fn run<M>(
    state: &mut EngineState,
    event: EngineEvent,
    res: &mut Resources<M>,
    looper: *mut ALooper,
) {
    for action in step(state, event) {
        apply_action(action, state, res, looper);
    }
}

/// 执行一个状态机动作。
///
/// `Exit` 不在此处处理：调用方通过 `EngineState::is_quitting()` 判断并退出循环
/// （保证已执行完同批的 `DestroySurface` / `DetachQueue`）。
fn apply_action<M>(
    action: EngineAction,
    state: &EngineState,
    res: &mut Resources<M>,
    looper: *mut ALooper,
) {
    match action {
        EngineAction::CreateSurface => {
            let (Some(vp), Some(window)) = (state.viewport(), res.window.as_ref()) else {
                log::error!("CreateSurface 但视口或窗口缺失");
                return;
            };
            match VelloRenderer::new(
                window,
                vp.width.max(0) as u32,
                vp.height.max(0) as u32,
                DisplayMetrics::from_viewport(&vp),
            ) {
                Ok(gpu) => {
                    log::info!("vello 渲染器已就绪");
                    res.renderer = Some(gpu);
                }
                Err(e) => {
                    log::error!("vello 渲染器初始化失败: {e}");
                    // 渲染器建不起来：窗口引用仍要保留（等销毁回调），
                    // 但本帧无 surface，draw_frame 会按 §3.4 丢弃重绘。
                }
            }
        }
        EngineAction::ResizeSurface => {
            let Some(vp) = state.viewport() else {
                return;
            };
            match res.renderer.as_mut() {
                Some(gpu) => {
                    log::info!("窗口 resize：{}x{}", vp.width, vp.height);
                    gpu.resize(vp.width.max(0) as u32, vp.height.max(0) as u32);
                }
                None => log::warn!("无渲染器，resize 只更新视口"),
            }
        }
        EngineAction::DestroySurface => {
            // 顺序固定：先停渲染（drop 渲染器释放 surface），再释放窗口引用。
            res.renderer = None;
            res.window = None;
            // 无窗口后不得再出帧，缓存的已布局树一并失效（§3.4）。
            res.frame = None;
            log::info!("surface 与窗口引用已释放");
        }
        EngineAction::AttachQueue => {
            let queue = res.pending_queue;
            res.pending_queue = ptr::null_mut();
            if queue.is_null() {
                log::warn!("AttachQueue 但队列指针为空");
                return;
            }
            attach_queue(&mut res.queue, looper, queue);
        }
        EngineAction::DetachQueue => detach_queue(&mut res.queue),
        EngineAction::Exit => {
            // 循环依据 is_quitting() 退出；此处仅留日志便于核对时序。
            log::info!("收到 Exit 动作，准备退出引擎循环");
        }
    }
}

/// 出帧：`on_draw → measure_and_layout → render`，并把**已布局**的树缓存进
/// `frame`（T13 的 hit-test 复用，避免一触重建两次，SPEC §7.7）。
///
/// 无窗口 / 无渲染器时按 §3.4 丢弃本次重绘，只记 warn。
fn draw_frame<A: Activity>(
    runtime: &mut ActivityRuntime<A>,
    renderer: &mut Option<crate::render::VelloRenderer>,
    viewport: Option<Viewport>,
    frame: &mut Option<View<A::Message>>,
) {
    let Some(vp) = viewport else {
        log::warn!("无窗口视口，丢弃本次重绘（§3.4）");
        return;
    };
    let Some(gpu) = renderer.as_mut() else {
        log::warn!("无可用渲染器，丢弃本次重绘（§3.4）");
        return;
    };
    let mut root = runtime.view();
    measure_and_layout_with(&mut root, &DisplayMetrics::from_viewport(&vp));
    log::info!("出帧：{}x{} density={:.2}", vp.width, vp.height, vp.density);
    gpu.render(&root);
    *frame = Some(root);
}

/// 在引擎线程把输入队列 attach 到本线程 Looper。
fn attach_queue(slot: &mut *mut AInputQueue, looper: *mut ALooper, queue: *mut AInputQueue) {
    if !slot.is_null() {
        log::warn!("新 InputQueue 创建时旧队列仍 attached，先 detach");
        detach_queue(slot);
    }
    // SAFETY: 仅在 Looper 所属线程调用；callback=None、ident>=0，
    // 事件就绪由 pollOnce 返回 ident。
    unsafe { AInputQueue_attachLooper(queue, looper, INPUT_QUEUE_IDENT, None, ptr::null_mut()) };
    *slot = queue;
    log::info!("InputQueue 已 attach 到引擎线程 Looper: {queue:p}");
}

/// 在引擎线程 detach（`AInputQueue_detachLooper` 同步保证返回后无事件分发）。
fn detach_queue(slot: &mut *mut AInputQueue) {
    let queue = *slot;
    if queue.is_null() {
        return;
    }
    // SAFETY: queue 由主线程回调保证在销毁 ack 期间仍然有效。
    unsafe { AInputQueue_detachLooper(queue) };
    *slot = ptr::null_mut();
    log::info!("InputQueue 已 detach: {queue:p}");
}

/// 取出并处理队列中当前所有待处理输入事件（T13：解码 → 拦截/hit-test → 入队）。
///
/// 每条取出的事件**恰好** `finishEvent` 一次（FR-I5）；唯一例外是
/// `preDispatchEvent` 返回非 0（事件已被 IME 接管），与 `android_native_app_glue`
/// 一致地直接放弃——此时 finish 会让 IME 丢事件。
fn drain_input<A: Activity>(
    queue: *mut AInputQueue,
    runtime: &mut ActivityRuntime<A>,
    state: &mut EngineState,
    res: &mut Resources<A::Message>,
) {
    loop {
        let mut event = ptr::null_mut();
        // SAFETY: queue 已 attach 且在引擎线程独占访问。
        let rc = unsafe { AInputQueue_getEvent(queue, &mut event) };
        if rc < 0 {
            break; // 无更多事件
        }

        // SAFETY: event 由 getEvent 借出，finish 前有效。
        let pre = unsafe { AInputQueue_preDispatchEvent(queue, event) };
        if pre != 0 {
            continue;
        }

        // SAFETY: 同上，from_ndk 只读取事件字段、不转移所有权。
        let handled = match unsafe { MotionEvent::from_ndk(event) } {
            Some(motion) => {
                handle_motion(runtime, state, res, &motion);
                // 触摸由框架消费：不再交回系统默认处理。
                1
            }
            // 非 motion（按键等）或 v1 不支持的 action：交回框架默认处理。
            None => 0,
        };

        // SAFETY: 每个 getEvent 取出的事件恰好 finish 一次。
        unsafe { AInputQueue_finishEvent(queue, event, handled) };
    }
}

/// 处理一次触控事件（FR-I1 ~ FR-I4）。
///
/// 顺序固定：**先**给 `Activity::on_touch_event`（开发者拦截优先，FR-I2），
/// 返回 `Some` 则不再走默认 hit-test；否则仅在 `ActionDown` 时做一次
/// hit-test（按住不连发，FR-I3）。
fn handle_motion<A: Activity>(
    runtime: &mut ActivityRuntime<A>,
    state: &mut EngineState,
    res: &mut Resources<A::Message>,
    motion: &MotionEvent,
) {
    log::info!(
        "[Input] action={:?} x={:.1} y={:.1}",
        motion.action,
        motion.x,
        motion.y
    );

    // 1) 拦截优先：所有 action 对 on_touch_event 可见（FR-I3），返回 Some
    //    即入队且跳过 hit-test（FR-I2）。
    if runtime.on_touch_event(motion) {
        note_message(state);
        return;
    }

    // 2) 默认点击语义：只在 DOWN 命中一次（FR-I1 / FR-I3）。
    if motion.action != TouchAction::ActionDown {
        return;
    }

    // 复用「当前帧已布局」的那棵树，**绝不**为此再调一次 on_draw——docs 的
    // 「ActionDown 分支单独 on_draw」会造成一触重建两次（§7.7 禁止）。
    let Some(frame) = res.frame.as_ref() else {
        log::warn!("[Input] 无已布局视图树，DOWN 事件丢弃");
        return;
    };
    match perform_hit_test(frame, motion.x, motion.y) {
        Some(message) => {
            runtime.enqueue(message);
            note_message(state);
        }
        // 空白 / 未绑定监听：不产生消息、不重绘（FR-I4）。
        None => log::info!("[Input] 未命中任何节点，不重绘"),
    }
}

/// 记录「产生了一条应用消息」：交给状态机决定是否置重绘标志（无窗口时丢弃，
/// §3.4）。`Message` 事件不产生动作，此处只借用它的判定逻辑。
fn note_message(state: &mut EngineState) {
    let actions = step(state, EngineEvent::Message);
    debug_assert!(actions.is_empty(), "Message 事件不应产生动作");
}

/// 在 C 回调边界捕获 panic（ADR-11）。
fn guard(f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        log::error!("NativeActivity 回调内发生 panic，已抑制");
    }
}

unsafe extern "C" fn native_window_created(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    guard(|| {
        log::info!("onNativeWindowCreated: window={window:p}");
        // SAFETY: 回调在主线程、activity/window 有效。
        let Some(ctx) = (unsafe { AppContext::borrow(activity) }) else {
            return;
        };
        if window.is_null() {
            return;
        }
        // acquire 与引擎线程 release 平衡：回调返回后框架可能回收其引用，
        // 引擎线程需自持一份（SPEC §3.3 资源所有权）。
        // SAFETY: window 在回调期间有效，acquire 仅增引用计数。
        unsafe { ANativeWindow_acquire(window) };
        // SAFETY: 同上，仅读取尺寸。
        let (width, height) = unsafe {
            (
                ANativeWindow_getWidth(window),
                ANativeWindow_getHeight(window),
            )
        };
        let density = read_density(activity);
        let msg = EngineMsg::WindowCreated {
            window: NdkPtr(window),
            width,
            height,
            density,
        };
        if !ctx.notify(msg) {
            // 引擎线程已退出：回滚本次 acquire，避免泄漏。
            // SAFETY: 平衡上面的 acquire，window 回调期间仍有效。
            unsafe { ANativeWindow_release(window) };
        }
    });
}

unsafe extern "C" fn native_window_destroyed(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    guard(|| {
        log::info!("onNativeWindowDestroyed: window={window:p}");
        // SAFETY: 回调在主线程、activity 有效。
        let Some(ctx) = (unsafe { AppContext::borrow(activity) }) else {
            return;
        };
        let (ack_tx, ack_rx) = unbounded();
        if ctx.notify(EngineMsg::WindowDestroyed(NdkPtr(window), ack_tx)) {
            // 阻塞到引擎线程释放渲染器与窗口引用后才返回——NDK 要求本回调
            // 返回后不再有任何线程使用该窗口（SPEC §3.3 同步销毁）。
            let _ = ack_rx.recv();
            log::info!("onNativeWindowDestroyed 同步 ack 已收到");
        }
    });
}

unsafe extern "C" fn native_window_resized(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    guard(|| {
        // SAFETY: 回调在主线程、activity/window 有效；resized 不改变窗口所有权，
        // 同一 ANativeWindow 仍由引擎线程持有，此处只读取新尺寸并通知，不 acquire。
        let Some(ctx) = (unsafe { AppContext::borrow(activity) }) else {
            return;
        };
        if window.is_null() {
            return;
        }
        // SAFETY: 回调期间 window 有效，仅读取尺寸。
        let (width, height) = unsafe {
            (
                ANativeWindow_getWidth(window),
                ANativeWindow_getHeight(window),
            )
        };
        log::info!("onNativeWindowResized: window={window:p} {width}x{height}");
        ctx.notify(EngineMsg::WindowResized { width, height });
    });
}

/// 从 AssetManager 配置读取屏幕 density（ADR-12：dpi/160，异常值兜底 1.0）。
fn read_density(activity: *mut ANativeActivity) -> f32 {
    // SAFETY: 回调期间 activity 与其 assetManager 有效。
    let asset_manager = unsafe { (*activity).assetManager };
    if asset_manager.is_null() {
        log::warn!("assetManager 为空，density 兜底 1.0");
        return 1.0;
    }
    // SAFETY: AConfiguration_new/delete 配对；fromAssetManager 只读借用 am。
    unsafe {
        let config = AConfiguration_new();
        if config.is_null() {
            log::warn!("AConfiguration_new 返回空，density 兜底 1.0");
            return 1.0;
        }
        AConfiguration_fromAssetManager(config, asset_manager);
        let dpi = AConfiguration_getDensity(config);
        AConfiguration_delete(config);

        if dpi > 0
            && dpi != ACONFIGURATION_DENSITY_ANY as i32
            && dpi != ACONFIGURATION_DENSITY_NONE as i32
        {
            dpi as f32 / 160.0
        } else {
            log::warn!("density dpi={dpi} 为异常值，兜底 1.0");
            1.0
        }
    }
}

unsafe extern "C" fn input_queue_created(activity: *mut ANativeActivity, queue: *mut AInputQueue) {
    guard(|| {
        log::info!("onInputQueueCreated: queue={queue:p}");
        // SAFETY: 回调在主线程、activity 有效。
        let Some(ctx) = (unsafe { AppContext::borrow(activity) }) else {
            return;
        };
        ctx.notify(EngineMsg::InputQueueCreated(NdkPtr(queue)));
    });
}

unsafe extern "C" fn input_queue_destroyed(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    guard(|| {
        log::info!("onInputQueueDestroyed: queue={queue:p}");
        // SAFETY: 回调在主线程、activity 有效。
        let Some(ctx) = (unsafe { AppContext::borrow(activity) }) else {
            return;
        };
        let (ack_tx, ack_rx) = unbounded();
        if ctx.notify(EngineMsg::InputQueueDestroyed(NdkPtr(queue), ack_tx)) {
            // 阻塞直到引擎线程完成 detach：NDK 要求该回调返回后
            // 不再有任何线程使用此队列（SPEC §3.3 同步销毁协议）。
            let _ = ack_rx.recv();
            log::info!("onInputQueueDestroyed 同步 ack 已收到");
        }
    });
}

unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    guard(|| {
        log::info!("onDestroy");
        // 取得上下文所有权（内部已清空 activity.instance，防止悬垂）。
        // SAFETY: 回调在主线程；同一 Activity 的 onDestroy 只触发一次。
        let Some(mut ctx) = (unsafe { AppContext::take(activity) }) else {
            log::warn!("onDestroy 时 activity.instance 为空，跳过");
            return;
        };
        ctx.shutdown();
        log::info!("引擎线程已 join，onDestroy 完成");
    });
}
