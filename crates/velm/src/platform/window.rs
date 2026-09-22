//! platform/window.rs — `ANativeWindow` 安全包装（SPEC §7.1、ADR-11/12）。
//!
//! 职责：持有窗口引用（acquire/release 配对）、读取尺寸与格式、配置缓冲几何
//! （RGBA_8888）、导出 raw-window-handle 0.6 句柄供 wgpu 建 surface。
//!
//! 本模块处于 FFI 边界，**仅在 Android 目标编译**；指针的所有权与释放时机由
//! `engine::activity_thread` 的销毁同步协议保证（§3.3）：引擎线程用完窗口后才
//! 回 ack，主线程才从销毁回调返回。

use std::ptr::NonNull;

use raw_ndk_sys::{
    ANativeWindow, ANativeWindow_LegacyFormat_WINDOW_FORMAT_RGBA_8888, ANativeWindow_acquire,
    ANativeWindow_getFormat, ANativeWindow_getHeight, ANativeWindow_getWidth,
    ANativeWindow_release, ANativeWindow_setBuffersGeometry,
};
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};

/// RGBA_8888：v1 唯一支持的缓冲格式（ADR-10）。
///
/// 绑定把 NDK 常量生成为 `u32`，而 `ANativeWindow_setBuffersGeometry` 的形参
/// 是 `i32`；常量值为 1，转换无损。
const FORMAT_RGBA_8888: i32 = ANativeWindow_LegacyFormat_WINDOW_FORMAT_RGBA_8888 as i32;

/// Android 窗口的安全包装：非空指针 + 明确的「是否持有 acquire 引用」状态。
///
/// 刻意**不实现 `Send`/`Sync`**：窗口只在引擎线程构造、使用与释放，禁止跨线程
/// 传递（跨线程只传裸指针，由 `activity_thread` 负责）。
pub struct NativeWindowWrapper {
    /// 窗口指针，构造即保证非空。
    ptr: NonNull<ANativeWindow>,
    /// 是否持有一个通过 `ANativeWindow_acquire` 得来的引用（Drop 时 release）。
    owned: bool,
}

impl NativeWindowWrapper {
    /// 包装一个 `ANativeWindow` 并 acquire 一个引用。
    ///
    /// # Safety
    /// - `ptr` 必须非空且指向有效的 `ANativeWindow`；
    /// - 调用方必须保证「本函数返回前该窗口不会被框架释放」。由 §3.3 的销毁
    ///   同步协议保证：引擎按 FIFO 先处理 `WindowCreated` 再处理
    ///   `WindowDestroyed`，且主线程在销毁回调里阻塞等引擎 ack，故 acquire
    ///   必然早于框架的最后一次 release。
    ///
    /// 返回 `None` 表示指针为空（不 panic，ADR-11）。
    pub unsafe fn from_ndk(ptr: *mut ANativeWindow) -> Option<Self> {
        let ptr = NonNull::new(ptr)?;
        // SAFETY: 由调用方保证 ptr 有效；acquire 增加一个引用，与 Drop 的
        // release 配对，使窗口在本包装存活期间不会被释放。
        unsafe { ANativeWindow_acquire(ptr.as_ptr()) };
        log::info!("[platform] ANativeWindow_acquire: {ptr:p}");
        Some(Self { ptr, owned: true })
    }

    /// 接手一个**已被调用方 acquire** 的窗口引用：本包装不再 acquire，Drop 时
    /// release 一次。
    ///
    /// 用于「主线程回调 acquire → 移交引擎线程」的所有权转移（§3.3）：引擎线程
    /// 处理 `WindowCreated` 时**不得**用 [`Self::from_ndk`]，否则会 acquire 第
    /// 二次，引用计数永不归零、窗口泄漏。
    ///
    /// # Safety
    /// - `ptr` 必须非空且指向有效的 `ANativeWindow`；
    /// - 调用方必须已为该指针 acquire 一次引用，并把「release 一次」的责任移交
    ///   本包装；此后调用方不得再 release 该引用。
    pub unsafe fn from_ndk_owned(ptr: *mut ANativeWindow) -> Option<Self> {
        let ptr = NonNull::new(ptr)?;
        log::info!("[platform] 接手已 acquire 的窗口引用: {ptr:p}");
        Some(Self { ptr, owned: true })
    }

    /// 原始指针（仅供 NDK 调用与日志；不得据此延长生命周期）。
    pub fn as_raw_ptr(&self) -> *mut ANativeWindow {
        self.ptr.as_ptr()
    }

    /// 是否持有 acquire 引用（Drop 时会 release）。
    pub fn is_owned(&self) -> bool {
        self.owned
    }

    /// 当前窗口尺寸 `(width, height)`（物理像素）。
    pub fn size(&self) -> (i32, i32) {
        // SAFETY: 包装存活期间窗口有效（acquire 引用 + 销毁同步）。
        unsafe {
            (
                ANativeWindow_getWidth(self.ptr.as_ptr()),
                ANativeWindow_getHeight(self.ptr.as_ptr()),
            )
        }
    }

    /// 当前缓冲格式（NDK 定义的像素格式枚举值）。
    pub fn format(&self) -> i32 {
        // SAFETY: 同上，仅读取。
        unsafe { ANativeWindow_getFormat(self.ptr.as_ptr()) }
    }

    /// 配置缓冲几何为 RGBA_8888（ADR-10 要求的唯一 v1 格式）。
    ///
    /// 失败时返回 NDK 的**负错误码**：调用方必须记日志并进入「无 surface」
    /// 状态，不得 panic（SPEC §7.1）。
    pub fn configure_buffers(&self, width: i32, height: i32) -> Result<(), i32> {
        // SAFETY: 包装存活期间窗口有效；setBuffersGeometry 只修改缓冲几何，
        // 不转移所有权，失败时窗口状态不变。
        let rc = unsafe {
            ANativeWindow_setBuffersGeometry(self.ptr.as_ptr(), width, height, FORMAT_RGBA_8888)
        };
        if rc < 0 {
            log::error!(
                "[platform] ANativeWindow_setBuffersGeometry({width}x{height}, RGBA_8888) 失败: {rc}"
            );
            Err(rc)
        } else {
            log::info!("[platform] 缓冲几何已配置: {width}x{height} RGBA_8888");
            Ok(())
        }
    }
}

impl HasWindowHandle for NativeWindowWrapper {
    /// 导出 rwh 0.6 窗口句柄，供 wgpu `create_surface_unsafe` 使用。
    ///
    /// 句柄带有生命周期：借用期间窗口必须保持有效——由 §3.3 的销毁同步保证
    /// （渲染器先 drop，再 release 窗口）。
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = AndroidNdkWindowHandle::new(self.ptr.cast());
        // SAFETY: 指针非空且在本包装存活期间有效；句柄不会延长窗口生命周期。
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(handle)) })
    }
}

impl HasDisplayHandle for NativeWindowWrapper {
    /// Android 的 display handle 是空结构体，无资源可泄漏。
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: AndroidDisplayHandle 不含任何指针，借用即有效。
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

impl Drop for NativeWindowWrapper {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: 该引用由 from_ndk 的 acquire 得来且尚未 release；
            // 调用方保证渲染器已先 drop（停止 present）后才释放窗口。
            unsafe { ANativeWindow_release(self.ptr.as_ptr()) };
            log::info!("[platform] ANativeWindow_release: {:p}", self.ptr);
        }
    }
}
