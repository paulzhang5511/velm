//! NativeActivity 入口与 C 回调绑定。
//!
//! T1（PLAN）：初始化日志、绑定 5 个必绑回调，回调体仅记录日志。
//! 引擎线程、Looper 与通道在 T3/T12 接入；渲染器在 T10/T11 接入。
//!
//! 安全设计（SPEC §3.3 / ADR-11）：
//! - 回调只做登记与日志，禁止阻塞或渲染；
//! - 所有跨 FFI 边界的回调统一经 [`callbacks::guard`] 捕获 panic，
//!   防止 unwind 进入 C 帧（未定义行为）；
//! - 回调内禁止 unwrap/expect。

use std::panic::{AssertUnwindSafe, catch_unwind};

use raw_ndk_sys::{AInputQueue, ANativeActivity, ANativeActivityCallbacks, ANativeWindow};

/// 日志 tag（logcat `-s VelmEngine`）。
const LOG_TAG: &str = "VelmEngine";

/// 由 cdylib 的 `ANativeActivity_onCreate` 调用：完成框架启动。
///
/// # Safety
/// `activity` 必须是 Android 框架传入的有效、非空、在本次 onCreate
/// 调用期间存活的 [`ANativeActivity`] 指针。`saved_state` 可为空；
/// 非空时其前 `saved_state_size` 字节必须可读（v1 不恢复状态，仅记录）。
pub unsafe fn bootstrap(
    activity: *mut ANativeActivity,
    saved_state: *mut core::ffi::c_void,
    saved_state_size: usize,
) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Trace)
                .with_tag(LOG_TAG),
        );

        log::info!(
            "ANativeActivity_onCreate: sdkVersion 见 activity, saved_state_size={}",
            saved_state_size
        );
        if !saved_state.is_null() {
            // v1 不支持状态恢复（SPEC §7.6 SavedInstanceState = P1）。
            log::warn!("收到 saved_state（{} 字节），v1 将忽略", saved_state_size);
        }

        if activity.is_null() || unsafe { (*activity).callbacks.is_null() } {
            log::error!("activity 或 callbacks 为空，无法绑定回调");
            return;
        }

        // SAFETY: 由调用契约保证 activity 与其 callbacks 在 onCreate
        // 期间有效且独占（框架在 onCreate 返回前不会从其他线程修改）。
        let callbacks: &mut ANativeActivityCallbacks = unsafe { &mut *(*activity).callbacks };
        callbacks.onNativeWindowCreated = Some(native_window_created);
        callbacks.onNativeWindowDestroyed = Some(native_window_destroyed);
        callbacks.onInputQueueCreated = Some(input_queue_created);
        callbacks.onInputQueueDestroyed = Some(input_queue_destroyed);
        callbacks.onDestroy = Some(on_destroy);

        log::info!("已绑定 5 个生命周期回调（window/input queue ×2 + destroy）");
    }));

    if result.is_err() {
        log::error!("bootstrap 发生 panic，已抑制以避免跨 FFI unwind");
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
        log::info!(
            "onNativeWindowCreated: activity={:p} window={:p}",
            activity,
            window
        );
    });
}

unsafe extern "C" fn native_window_destroyed(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    guard(|| {
        log::info!(
            "onNativeWindowDestroyed: activity={:p} window={:p}",
            activity,
            window
        );
    });
}

unsafe extern "C" fn input_queue_created(activity: *mut ANativeActivity, queue: *mut AInputQueue) {
    guard(|| {
        log::info!(
            "onInputQueueCreated: activity={:p} queue={:p}",
            activity,
            queue
        );
    });
}

unsafe extern "C" fn input_queue_destroyed(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    guard(|| {
        log::info!(
            "onInputQueueDestroyed: activity={:p} queue={:p}",
            activity,
            queue
        );
    });
}

unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    guard(|| {
        log::info!("onDestroy: activity={:p}", activity);
    });
}
