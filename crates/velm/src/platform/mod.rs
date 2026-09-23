//! platform/ — 平台层：屏幕配置（host 可测）+ ANativeWindow 包装（android-only）。
//!
//! 与 `engine` 同样的 gate 约定：本模块**在 host 可见**，但只有纯逻辑部分会
//! 真正编译进来——
//!
//! - 本文件：`ScreenConfig` 与 `density_from_dpi`，纯算术，host 可直接单测（§10.2）；
//! - `window`：处于 FFI 边界，仅在 Android 目标编译，host 上不存在。
//!
//! 因此 `platform` 不能在 lib.rs 里整块 `#[cfg(target_os = "android")]` 关掉，
//! 否则 density 换算会失去 host 可测性——android-only 的部分在**子模块**上 gate。

/// 1x（mdpi）基准密度：density = dpi / 160（ADR-12）。
pub const DENSITY_MEDIUM_DPI: i32 = 160;
/// `ACONFIGURATION_DENSITY_ANY`：系统未限定密度（资源的通配符值，非真实 dpi）。
const DENSITY_ANY: i32 = 65534;
/// `ACONFIGURATION_DENSITY_NONE`：无密度信息。
const DENSITY_NONE: i32 = 65535;

/// 编译期校验：手写常量必须与 raw-ndk-sys 绑定一致，防止常量漂移
/// （raw-ndk-sys 是 android-only 依赖，故 host 上跳过）。
#[cfg(target_os = "android")]
const _: () = {
    assert!(DENSITY_MEDIUM_DPI == raw_ndk_sys::ACONFIGURATION_DENSITY_MEDIUM as i32);
    assert!(DENSITY_ANY == raw_ndk_sys::ACONFIGURATION_DENSITY_ANY as i32);
    assert!(DENSITY_NONE == raw_ndk_sys::ACONFIGURATION_DENSITY_NONE as i32);
};

/// 屏幕配置：窗口物理像素尺寸 + density（供 `layout::measure_and_layout` 使用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenConfig {
    /// 窗口宽度（物理像素）。
    pub width: i32,
    /// 窗口高度（物理像素）。
    pub height: i32,
    /// 密度（dpi/160）；异常或不可得时为 1.0。
    pub density: f32,
}

/// 把 `AConfiguration_getDensity` 的 dpi 换算为 density（ADR-12）。
///
/// 纯函数、host 可测：`ANY`/`NONE` 两个通配符与非法值（≤0）一律兜底 `1.0`，
/// 由调用方决定是否打 warn。返回 `1.0` 意味着「按 mdpi 处理」而非「出错」，
/// 调用方不需要据此中断渲染。
pub fn density_from_dpi(dpi: i32) -> f32 {
    if dpi > 0 && dpi != DENSITY_ANY && dpi != DENSITY_NONE {
        dpi as f32 / DENSITY_MEDIUM_DPI as f32
    } else {
        1.0
    }
}

/// Android 风格密度模型（density bucket / sp-dp 分离 / 像素取整，host 可测）。
pub mod display_metrics;

pub use display_metrics::{DensityBucket, DisplayMetrics};

#[cfg(target_os = "android")]
pub mod window;
