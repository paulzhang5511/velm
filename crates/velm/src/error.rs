//! error.rs — 框架统一错误类型（ADR-11：错误可诊断，绝不 panic）。
//!
//! v1 只有渲染器初始化会「失败并可恢复」（窗口/适配器/设备），绘制失败一律
//! 记日志并跳过该帧（不返回错误），故本枚举**只覆盖初始化路径**，不包含
//! 运行时每帧的错误——避免把每帧的告警噪音变成控制流。
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

    /// vello 渲染器初始化失败（shader 编译等）。
    #[error("初始化 vello 渲染器失败: {0}")]
    VelloInit(String),
}

/// 框架统一返回类型。
pub type Result<T> = std::result::Result<T, Error>;
