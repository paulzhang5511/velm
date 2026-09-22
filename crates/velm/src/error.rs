//! error.rs — 框架统一错误类型（ADR-11：错误可诊断，绝不 panic）。
//!
//! 只有渲染器**初始化路径**会「失败并可恢复」（surface / 适配器 / 设备），
//! 绘制失败一律记日志并跳过该帧（不返回错误），故本枚举**只覆盖初始化路径**，
//! 不包含运行时每帧的错误——避免把每帧的告警噪音变成控制流。
//!
//! vello_gpu 0.2 的 `Renderer::new` 返回 `(Renderer, Resources)` 且不可失败，
//! 因此渲染器创建本身不会进入本枚举；可失败的只有 wgpu 设备/适配器/surface 阶段。
//!
//! 变体字段一律用 `String` / 基本类型，不引用 `wgpu` 等 android-only 类型，
//! 使本模块在 host 上也可编译（SPEC §10.2）。

use thiserror::Error;

/// 框架错误类型。
#[derive(Debug, Error)]
pub enum Error {
    /// 窗口宽高为 0（surface 无法配置）。
    #[error("窗口尺寸非法: {0}x{1}（宽高必须非 0）")]
    InvalidWindowSize(u32, u32),

    /// `NativeWindowWrapper` 导不出 rwh 0.6 句柄（理论上不会发生）。
    #[error("窗口句柄不可用（raw-window-handle 导出失败）")]
    WindowHandleUnavailable,

    /// `ANativeWindow_setBuffersGeometry` 返回负错误码。
    #[error("配置窗口缓冲几何失败（NDK 错误码 {0}）")]
    BufferGeometry(i32),

    /// `Instance::create_surface_unsafe` 失败（surface 不被任何后端支持）。
    #[error("创建 wgpu surface 失败: {0}")]
    CreateSurface(String),

    /// 找不到与 surface 兼容的 Vulkan 适配器。
    #[error("无可用 GPU 适配器（backends = VULKAN）")]
    NoAdapter,

    /// `Adapter::request_device` 失败。
    #[error("请求 wgpu 设备失败: {0}")]
    RequestDevice(String),

    /// `Surface::get_default_config` 返回 `None`：窗口与适配器无兼容配置。
    #[error("窗口与适配器无兼容的 surface 配置")]
    NoSurfaceConfig,
}

/// 框架统一返回类型。
pub type Result<T> = std::result::Result<T, Error>;
