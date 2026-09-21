//! Counter 示例应用（cdylib，由 android.app.NativeActivity 加载）。
//!
//! T1 阶段：仅导出 `ANativeActivity_onCreate` 并验证回调链；
//! TEA 计数器逻辑在 T13 接入。

/// NativeActivity 入口（native_activity.h 要求的唯一导出符号）。
///
/// # Safety
/// 由 Android 框架调用：`activity` 为有效的 `ANativeActivity*`；
/// `saved_state` 可为空，非空时前 `saved_state_size` 字节可读。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut raw_ndk_sys::ANativeActivity,
    saved_state: *mut std::ffi::c_void,
    saved_state_size: usize,
) {
    // 入口极薄：所有权与逻辑全部在框架内（SPEC §7.7）。
    unsafe { velm::engine::activity_thread::bootstrap(activity, saved_state, saved_state_size) };
}
