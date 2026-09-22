//! engine/app_context.rs — Activity 上下文句柄的生命周期（SPEC §3.3 / §7.7）。
//!
//! Android 把「每 Activity 一份的框架状态」存放在 `ANativeActivity::instance`
//! 的裸指针里。本模块把这份状态的**所有权规则**收敛成四个操作：
//!
//! - [`AppContext::install`]：onCreate 用 `Box::into_raw` 发布（`Box` 所有权
//!   移交给 `activity.instance`）；
//! - [`AppContext::borrow`]：5 个生命周期回调只借用（主线程串行触发，不存在
//!   并发释放）；
//! - [`AppContext::take`]：onDestroy 取得所有权、立即清空 `instance` 字段
//!   （防止悬垂），随后 join 引擎线程；
//! - [`AppContext::shutdown`]：发 Quit → 唤醒 Looper → join。
//!
//! 借此保证「20 次启停」压测下既不泄漏也不 use-after-free：句柄有且只有一个
//! 所有者，取走即置空。
//!
//! 本模块处于 FFI 边界，**仅在 Android 目标编译**。

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use raw_ndk_sys::{AInputQueue, ALooper, ALooper_wake, ANativeActivity, ANativeWindow};

/// 包装 NDK 裸指针以跨线程传递（裸指针默认非 `Send`）。
///
/// 安全性由协议而非类型保证：指针所指对象的**引用计数**已在回调侧 acquire，
/// 引擎线程接手后才 release（见 `platform::window::NativeWindowWrapper`）。
pub(crate) struct NdkPtr<T>(pub(crate) *mut T);

// SAFETY: NDK 保证 `AInputQueue` 在 `onInputQueueDestroyed` 回调返回前有效；
// 该队列只在引擎线程被 attach/访问，销毁经 detach + 同步 ack，不存在并发访问。
unsafe impl<T> Send for NdkPtr<T> {}

/// 主线程 → 引擎线程的控制消息。
pub(crate) enum EngineMsg {
    InputQueueCreated(NdkPtr<AInputQueue>),
    /// 第二参数为一次性 ack 通道：引擎完成 detach 后回执，主线程才放行。
    InputQueueDestroyed(NdkPtr<AInputQueue>, Sender<()>),
    /// 窗口引用随消息移交：回调侧已 acquire，引擎线程负责 release。
    WindowCreated {
        window: NdkPtr<ANativeWindow>,
        width: i32,
        height: i32,
        density: f32,
    },
    /// 引擎停止使用窗口并 release 后回执，主线程才从销毁回调返回。
    WindowDestroyed(NdkPtr<ANativeWindow>, Sender<()>),
    /// 同一窗口尺寸变化（旋转 / 分屏）：指针与所有权不变。
    WindowResized {
        width: i32,
        height: i32,
    },
    Quit,
}

/// 框架的 Activity 上下文：与引擎线程通信的通道、Looper 与线程句柄。
pub(crate) struct AppContext {
    tx: Sender<EngineMsg>,
    /// 引擎线程 Looper，prepare 完成后注册（供主线程 `ALooper_wake`）。
    looper: Arc<AtomicPtr<ALooper>>,
    join: Option<JoinHandle<()>>,
}

impl AppContext {
    /// 把上下文发布到 `activity.instance`。
    ///
    /// # Safety
    /// `activity` 必须非空且独占可写（onCreate 期间成立）。
    pub(crate) unsafe fn install(
        activity: *mut ANativeActivity,
        tx: Sender<EngineMsg>,
        looper: Arc<AtomicPtr<ALooper>>,
        join: JoinHandle<()>,
    ) -> bool {
        if activity.is_null() {
            return false;
        }
        let ctx = Box::new(Self {
            tx,
            looper,
            join: Some(join),
        });
        // SAFETY: 契约保证 activity 非空且未被写入过；Box 的所有权移交给
        // activity.instance，由 onDestroy 的 take 回收。
        unsafe { (*activity).instance = Box::into_raw(ctx) as *mut c_void };
        true
    }

    /// 借用上下文（生命周期回调用）。
    ///
    /// # Safety
    /// `activity` 必须非空，且回调期间 `instance` 不会被取走（回调均在主线程
    /// 串行触发，只有 onDestroy 会取走）。
    pub(crate) unsafe fn borrow(activity: *mut ANativeActivity) -> Option<&'static Self> {
        if activity.is_null() {
            return None;
        }
        // SAFETY: 同上，instance 字段可读。
        let ptr = unsafe { (*activity).instance } as *const Self;
        if ptr.is_null() {
            None
        } else {
            // SAFETY: 借用期内 Box 不被释放（见函数 Safety）。
            Some(unsafe { &*ptr })
        }
    }

    /// 取回所有权并**立即清空** `activity.instance`（onDestroy 用）。
    ///
    /// # Safety
    /// `activity` 必须非空；同一 Activity 只能调用一次（否则重复回收）。
    pub(crate) unsafe fn take(activity: *mut ANativeActivity) -> Option<Box<Self>> {
        if activity.is_null() {
            return None;
        }
        // SAFETY: 同上，instance 字段可读可写。
        let ptr = unsafe { (*activity).instance } as *mut Self;
        if ptr.is_null() {
            return None;
        }
        // 先置空再回收：即便后续回调（理论上不会有）触发也只会得到 None。
        // SAFETY: 同上。
        unsafe { (*activity).instance = ptr::null_mut() };
        // SAFETY: Box 由 install 的 into_raw 发布，此处唯一一次回收。
        Some(unsafe { Box::from_raw(ptr) })
    }

    /// 发一条控制消息并唤醒引擎线程；返回 `false` 表示引擎线程已退出。
    pub(crate) fn notify(&self, msg: EngineMsg) -> bool {
        if self.tx.send(msg).is_err() {
            log::warn!("[Engine] 引擎线程已退出，控制消息丢弃");
            return false;
        }
        self.wake();
        true
    }

    /// 唤醒引擎线程 Looper（控制消息入队后调用）。
    fn wake(&self) {
        let looper = self.looper.load(Ordering::Acquire);
        if !looper.is_null() {
            // SAFETY: looper 由引擎线程 prepare 获得；onDestroy 先发 Quit/wake
            // 再 join，故此处不可能晚于引擎线程退出。
            unsafe { ALooper_wake(looper) };
        }
    }

    /// 停止引擎：发 Quit → 唤醒 → join。
    ///
    /// 即便 Quit 发送失败（引擎线程已 panic 退出）也照常 join，避免线程泄漏。
    pub(crate) fn shutdown(&mut self) {
        if self.tx.send(EngineMsg::Quit).is_err() {
            log::warn!("[Engine] Quit 发送失败，直接 join（引擎线程可能已退出）");
        }
        self.wake();
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("[Engine] 引擎线程 panic 退出");
        }
    }

    /// 引擎线程注册自己的 Looper（供主线程唤醒）。
    pub(crate) fn publish_looper(looper: &Arc<AtomicPtr<ALooper>>, ptr: *mut ALooper) {
        looper.store(ptr, Ordering::Release);
    }
}

/// 空 Looper 指针常量：避免各回调重复写 `ptr::null_mut()`。
pub(crate) fn null_looper_slot() -> Arc<AtomicPtr<ALooper>> {
    Arc::new(AtomicPtr::new(ptr::null_mut()))
}
