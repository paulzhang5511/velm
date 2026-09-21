//! NativeActivity 入口、C 回调绑定与引擎线程（T3 spike 版）。
//!
//! 事件模型（SPEC §3.3 / ADR-09）：
//! - 主线程（UI）的 C 回调只做登记与 channel 发布，绝不阻塞渲染或轮询；
//! - 引擎线程持有自己的 [`ALooper`]，输入队列只在该线程 attach/detach；
//! - 队列销毁走同步 ack：主线程回调阻塞到引擎线程完成 detach 才返回；
//! - onDestroy 发 Quit 并 join 引擎线程。
//!
//! T3 为 spike：输入事件仅记录日志并 finish（按键交回框架默认处理，
//! 触摸先消费）；TEA 分发在 T13 接入，窗口 acquire/release 在本任务
//! Slice 3 接入。

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};
use raw_ndk_sys::{
    AINPUT_EVENT_TYPE_KEY, AINPUT_EVENT_TYPE_MOTION, AInputEvent_getType, AInputQueue,
    AInputQueue_attachLooper, AInputQueue_detachLooper, AInputQueue_finishEvent,
    AInputQueue_getEvent, AInputQueue_preDispatchEvent, AKeyEvent_getKeyCode, ALOOPER_POLL_ERROR,
    ALOOPER_PREPARE_ALLOW_NON_CALLBACKS, ALooper, ALooper_pollOnce, ALooper_prepare, ALooper_wake,
    AMotionEvent_getAction, AMotionEvent_getX, AMotionEvent_getY, ANativeActivity,
    ANativeActivityCallbacks, ANativeWindow, ANativeWindow_Buffer,
    ANativeWindow_LegacyFormat_WINDOW_FORMAT_RGBA_8888, ANativeWindow_lock,
    ANativeWindow_setBuffersGeometry, ANativeWindow_unlockAndPost,
};

/// 日志 tag（logcat `-s VelmEngine`）。
const LOG_TAG: &str = "VelmEngine";
/// attachLooper 的 looper ident；pollOnce 返回该值表示输入队列有事件。
const INPUT_QUEUE_IDENT: i32 = 1;
/// pollOnce 超时兜底（毫秒，ADR-09）。
const POLL_TIMEOUT_MS: i32 = 16;

/// 包装 NDK 裸指针以跨线程传递（裸指针默认非 `Send`）。
struct NdkPtr<T>(*mut T);

// SAFETY: NDK 保证 `AInputQueue` 在 `onInputQueueDestroyed` 回调返回前有效；
// 该队列只在引擎线程被 attach/访问，销毁经 detach + 同步 ack，不存在并发访问。
unsafe impl<T> Send for NdkPtr<T> {}

/// 主线程 → 引擎线程的控制消息。
enum EngineMsg {
    InputQueueCreated(NdkPtr<AInputQueue>),
    /// 第二参数为一次性 ack 通道：引擎完成 detach 后回执，主线程才放行。
    InputQueueDestroyed(NdkPtr<AInputQueue>, Sender<()>),
    Quit,
}

/// 引擎句柄，生命周期与 Activity 实例一致（裸指针存于 `activity.instance`）。
struct Engine {
    tx: Sender<EngineMsg>,
    /// 引擎线程 Looper，prepare 完成后注册（供主线程 `ALooper_wake`）。
    looper: Arc<AtomicPtr<ALooper>>,
    join: Option<JoinHandle<()>>,
}

/// 由 cdylib 的 `ANativeActivity_onCreate` 调用：完成框架启动。
///
/// # Safety
/// `activity` 必须是 Android 框架传入的有效、非空、在本次 onCreate
/// 调用期间存活的 [`ANativeActivity`] 指针。`saved_state` 可为空；
/// 非空时其前 `saved_state_size` 字节必须可读（v1 不恢复状态，仅记录）。
pub unsafe fn bootstrap(
    activity: *mut ANativeActivity,
    saved_state: *mut c_void,
    saved_state_size: usize,
) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Trace)
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
        let looper = Arc::new(AtomicPtr::new(ptr::null_mut()));

        let join = match thread::Builder::new().name("velm-engine".into()).spawn({
            let looper = looper.clone();
            move || engine_main(rx, looper)
        }) {
            Ok(j) => j,
            Err(e) => {
                log::error!("引擎线程启动失败: {e}");
                return;
            }
        };

        let engine = Box::new(Engine {
            tx,
            looper,
            join: Some(join),
        });
        // SAFETY: 契约保证 activity 在 onCreate 期间有效独占。
        unsafe { (*activity).instance = Box::into_raw(engine) as *mut c_void };

        // SAFETY: 同上，callbacks 在 Activity 生命周期内有效。
        let callbacks: &mut ANativeActivityCallbacks = unsafe { &mut *(*activity).callbacks };
        callbacks.onNativeWindowCreated = Some(native_window_created);
        callbacks.onNativeWindowDestroyed = Some(native_window_destroyed);
        callbacks.onInputQueueCreated = Some(input_queue_created);
        callbacks.onInputQueueDestroyed = Some(input_queue_destroyed);
        callbacks.onDestroy = Some(on_destroy);

        log::info!("已绑定 5 个生命周期回调，引擎线程已启动");
    }));

    if result.is_err() {
        log::error!("bootstrap 发生 panic，已抑制以避免跨 FFI unwind");
    }
}

/// 引擎线程主函数：prepare Looper、attach 输入队列、轮询分发。
fn engine_main(rx: Receiver<EngineMsg>, looper_slot: Arc<AtomicPtr<ALooper>>) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 本线程首次 prepare，返回与本线程绑定的 Looper；
        // ALLOW_NON_CALLBACKS 允许 pollOnce 直接返回 fd ident。
        let looper = unsafe { ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as i32) };
        if looper.is_null() {
            log::error!("ALooper_prepare 返回空，引擎线程退出");
            return;
        }
        looper_slot.store(looper, Ordering::Release);
        log::info!("引擎线程 Looper 就绪");

        let mut queue: *mut AInputQueue = ptr::null_mut();

        'outer: loop {
            // 先排空控制通道，保证销毁/退出消息优先于输入处理。
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    EngineMsg::InputQueueCreated(NdkPtr(q)) => {
                        attach_queue(&mut queue, looper, q);
                    }
                    EngineMsg::InputQueueDestroyed(NdkPtr(q), ack) => {
                        if queue == q {
                            detach_queue(&mut queue);
                        } else {
                            log::warn!("QueueDestroyed 与当前 attached 队列不一致");
                        }
                        // 无论是否匹配都回执：回调必须被放行。
                        let _ = ack.send(());
                    }
                    EngineMsg::Quit => {
                        if !queue.is_null() {
                            detach_queue(&mut queue);
                        }
                        break 'outer;
                    }
                }
            }

            // SAFETY: 本线程持有 looper；空指针出参表示不取 fd/events/data。
            let poll_rc = unsafe {
                ALooper_pollOnce(
                    POLL_TIMEOUT_MS,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };

            if poll_rc == INPUT_QUEUE_IDENT && !queue.is_null() {
                drain_input(queue);
            } else if poll_rc == ALOOPER_POLL_ERROR {
                log::error!("ALooper_pollOnce 返回 ERROR，引擎线程退出");
                break;
            }
            // WAKE(-1) / TIMEOUT(-3) / CALLBACK(-2，未用回调) 均回到顶部收消息。
        }
    }));

    if outcome.is_err() {
        log::error!("引擎线程发生 panic，受控退出");
    }
    log::info!("引擎线程已退出");
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

/// 取出并处理队列中当前所有待处理输入事件（spike：日志 + finish）。
fn drain_input(queue: *mut AInputQueue) {
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
            // IME 等预派发已接管：按 NDK 契约放弃本轮处理且不 finish。
            continue;
        }

        // SAFETY: 同上；仅读取事件字段。
        let handled = unsafe {
            let event_type = AInputEvent_getType(event);
            if event_type == AINPUT_EVENT_TYPE_KEY as i32 {
                let key_code = AKeyEvent_getKeyCode(event);
                log::info!("KeyEvent: keyCode={key_code}（handled=0，交回框架默认处理）");
                0
            } else if event_type == AINPUT_EVENT_TYPE_MOTION as i32 {
                let action = AMotionEvent_getAction(event) & 0xff;
                let x = AMotionEvent_getX(event, 0);
                let y = AMotionEvent_getY(event, 0);
                log::info!("MotionEvent: action={action} x={x:.1} y={y:.1}（spike 暂消费）");
                1
            } else {
                0
            }
        };

        // SAFETY: 每个 getEvent 取出的事件恰好 finish 一次。
        unsafe { AInputQueue_finishEvent(queue, event, handled) };
    }
}

/// 从 `activity.instance` 借用引擎句柄。
///
/// # Safety
/// 调用方须保证处于 Activity 生命周期内、instance 由 bootstrap 发布且
/// onDestroy 已取得所有权前不被并发释放（回调均在主线程串行触发）。
unsafe fn engine_of(activity: *mut ANativeActivity) -> Option<&'static Engine> {
    // SAFETY: 契约保证 activity 非空且 instance 字段可读。
    let ptr = unsafe { (*activity).instance } as *mut Engine;
    if ptr.is_null() {
        None
    } else {
        // SAFETY: 见函数 Safety；主线程回调期间 Box 不被释放。
        Some(unsafe { &*ptr })
    }
}

/// 唤醒引擎线程 Looper（控制消息入队后调用）。
fn wake_engine(engine: &Engine) {
    let looper = engine.looper.load(Ordering::Acquire);
    if !looper.is_null() {
        // SAFETY: looper 由引擎线程 prepare 获得；onDestroy 先发 Quit/wake
        // 再 join，故此处不可能晚于引擎线程退出。
        unsafe { ALooper_wake(looper) };
    }
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
        // T3 POC：提交一帧纯色，验证"surface 首帧建立输入窗口几何"假设；
        // T10 vello 渲染器接入后删除（Slice 3 再做 acquire/release 所有权）。
        post_probe_frame(window);
        let _ = activity;
    });
}

/// T3 spike 临时代码：软件 lock 一帧纯色并 post，触发 surface 首帧
/// （未提交任何 buffer 时 InputWindowHandle 的 frame 为 0×0，触摸不投递）。
fn post_probe_frame(window: *mut ANativeWindow) {
    if window.is_null() {
        return;
    }
    // SAFETY: window 指针由本次 onNativeWindowCreated 回调提供，调用期间有效。
    unsafe {
        let geo = ANativeWindow_setBuffersGeometry(
            window,
            0,
            0,
            ANativeWindow_LegacyFormat_WINDOW_FORMAT_RGBA_8888 as i32,
        );
        if geo != 0 {
            log::warn!("setBuffersGeometry 失败: {geo}，按默认格式继续");
        }

        let mut buffer = ANativeWindow_Buffer::default();
        let rc = ANativeWindow_lock(window, &mut buffer, ptr::null_mut());
        if rc != 0 {
            log::warn!("ANativeWindow_lock 失败: {rc}");
            return;
        }

        let (w, h, stride) = (
            buffer.width as usize,
            buffer.height as usize,
            buffer.stride as usize,
        );
        if !buffer.bits.is_null() && w > 0 && h > 0 && stride >= w {
            // RGBA_8888 内存序 R,G,B,A；小端 u32 = 0xAABBGGRR，#121212。
            let pixels = std::slice::from_raw_parts_mut(buffer.bits as *mut u32, stride * h);
            pixels.fill(0xFF121212);
        }

        let posted = ANativeWindow_unlockAndPost(window);
        if posted == 0 {
            log::info!("POC 首帧已提交: {w}x{h} stride={stride}");
        } else {
            log::warn!("ANativeWindow_unlockAndPost 失败: {posted}");
        }
    }
}

unsafe extern "C" fn native_window_destroyed(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    guard(|| {
        log::info!("onNativeWindowDestroyed: window={window:p}");
        // Slice 3：同步停绘 + release。
        let _ = activity;
    });
}

unsafe extern "C" fn input_queue_created(activity: *mut ANativeActivity, queue: *mut AInputQueue) {
    guard(|| {
        log::info!("onInputQueueCreated: queue={queue:p}");
        // SAFETY: 回调在主线程、activity 有效。
        let Some(engine) = (unsafe { engine_of(activity) }) else {
            return;
        };
        if engine
            .tx
            .send(EngineMsg::InputQueueCreated(NdkPtr(queue)))
            .is_ok()
        {
            wake_engine(engine);
        }
    });
}

unsafe extern "C" fn input_queue_destroyed(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    guard(|| {
        log::info!("onInputQueueDestroyed: queue={queue:p}");
        // SAFETY: 回调在主线程、activity 有效。
        let Some(engine) = (unsafe { engine_of(activity) }) else {
            return;
        };
        let (ack_tx, ack_rx) = unbounded();
        if engine
            .tx
            .send(EngineMsg::InputQueueDestroyed(NdkPtr(queue), ack_tx))
            .is_ok()
        {
            wake_engine(engine);
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
        // SAFETY: 回调在主线程；此处取得 Engine 所有权并回收 Box。
        let ptr = unsafe { (*activity).instance } as *mut Engine;
        if ptr.is_null() {
            return;
        }
        // SAFETY: bootstrap 用 Box::into_raw 发布，onDestroy 仅一次回收。
        let mut engine = unsafe { Box::from_raw(ptr) };
        // SAFETY: 回收后立即清空 instance，防止悬垂。
        unsafe { (*activity).instance = ptr::null_mut() };

        if engine.tx.send(EngineMsg::Quit).is_ok() {
            wake_engine(&engine);
        }
        if let Some(join) = engine.join.take()
            && join.join().is_err()
        {
            log::error!("引擎线程 panic 退出");
        }
        log::info!("引擎线程已 join，onDestroy 完成");
    });
}
