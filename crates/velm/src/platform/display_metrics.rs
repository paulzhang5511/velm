//! platform/display_metrics.rs — Android 风格密度模型（host 可测，无 android 符号）。
//!
//! 把裸 `density: f32` 升级为 Android 的完整密度概念（参考 `AConfiguration_getDensity`
//! 与 `DisplayMetrics` / `TypedValue`）：
//!
//! - `density` = dpi / 160（与 ADR-12 一致）；
//! - `scaled_density` = `density * font_scale`，专用于 **sp**（字体随系统「字体大小」
//!   设置缩放；dp 不随它缩放）；
//! - `DensityBucket`：ldpi~xxxhdpi 六个标准桶，用于资源限定与文档化档位；
//! - dp/sp→px 统一换算，并对最终物理像素**四舍五入取整**（Android 的
//!   `TypedValue.complexToDimensionPixelSize` 行为），保证不同 density 下渲染清晰、
//!   几何与真机一致。
//!
//! 一切换算必须经由本结构体，禁止在布局 / 渲染里散落 `value * 2.0` 之类的硬编码。

use crate::engine::events::Viewport;

/// 标准密度桶（对应 Android `res/values-*dpi` 的资源桶划分）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DensityBucket {
    /// ldpi：约 120dpi，scale 0.75。
    Ldpi,
    /// mdpi：160dpi（基准），scale 1.0。
    Mdpi,
    /// hdpi：240dpi，scale 1.5。
    Hdpi,
    /// xhdpi：320dpi，scale 2.0。
    Xhdpi,
    /// xxhdpi：480dpi，scale 3.0。
    Xxhdpi,
    /// xxxhdpi：640dpi，scale 4.0。
    Xxxhdpi,
    /// 非标准密度，保留原始 scale 供参考。
    Other(f32),
}

impl DensityBucket {
    /// 由 density（dpi/160）选取最接近的桶。
    pub fn from_density(density: f32) -> Self {
        if !density.is_finite() || density <= 0.0 {
            return DensityBucket::Mdpi;
        }
        match density {
            d if (d - 0.75).abs() < 1e-3 => DensityBucket::Ldpi,
            d if (d - 1.0).abs() < 1e-3 => DensityBucket::Mdpi,
            d if (d - 1.5).abs() < 1e-3 => DensityBucket::Hdpi,
            d if (d - 2.0).abs() < 1e-3 => DensityBucket::Xhdpi,
            d if (d - 3.0).abs() < 1e-3 => DensityBucket::Xxhdpi,
            d if (d - 4.0).abs() < 1e-3 => DensityBucket::Xxxhdpi,
            d => DensityBucket::Other(d),
        }
    }

    /// 该桶对应的 density scale。
    pub fn scale(&self) -> f32 {
        match self {
            DensityBucket::Ldpi => 0.75,
            DensityBucket::Mdpi => 1.0,
            DensityBucket::Hdpi => 1.5,
            DensityBucket::Xhdpi => 2.0,
            DensityBucket::Xxhdpi => 3.0,
            DensityBucket::Xxxhdpi => 4.0,
            DensityBucket::Other(d) => *d,
        }
    }
}

/// Android 风格的屏幕密度与尺寸描述。
///
/// 持有窗口物理像素尺寸（与 `MotionEvent` / 布局坐标系一致，ADR-12）以及完整密度
/// 信息。所有 dp/sp→px 换算均经本结构体，以确保 sp 与 dp 区分、像素取整一致。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayMetrics {
    /// 窗口宽度（物理像素）。
    pub width_px: f32,
    /// 窗口高度（物理像素）。
    pub height_px: f32,
    /// 基准密度（dpi/160）。
    pub density: f32,
    /// 字体缩放系数（系统「显示大小 / 字体大小」设置）；sp 经它缩放。
    pub font_scale: f32,
    /// 缩放后的密度：`density * font_scale`，专用于 sp。
    pub scaled_density: f32,
    /// 当前密度落入的标准桶。
    pub bucket: DensityBucket,
}

impl DisplayMetrics {
    /// 以基准密度构造；`font_scale` 默认 1.0（不随系统字体缩放）。
    pub fn new(width_px: f32, height_px: f32, density: f32) -> Self {
        Self::with_font_scale(width_px, height_px, density, 1.0)
    }

    /// 显式指定字体缩放系数（系统设置注入处）。
    pub fn with_font_scale(width_px: f32, height_px: f32, density: f32, font_scale: f32) -> Self {
        let density = if density.is_finite() && density > 0.0 {
            density
        } else {
            1.0
        };
        let font_scale = if font_scale.is_finite() && font_scale > 0.0 {
            font_scale
        } else {
            1.0
        };
        let scaled_density = density * font_scale;
        Self {
            width_px,
            height_px,
            density,
            font_scale,
            scaled_density,
            bucket: DensityBucket::from_density(density),
        }
    }

    /// 由 dpi（NDK `AConfiguration_getDensity` 原始值）构造。
    pub fn from_dpi(width_px: f32, height_px: f32, dpi: i32, font_scale: f32) -> Self {
        Self::with_font_scale(
            width_px,
            height_px,
            crate::platform::density_from_dpi(dpi),
            font_scale,
        )
    }

    /// 由引擎视口构造（沿用视口里的 density；font_scale 默认 1.0，待系统设置注入）。
    pub fn from_viewport(vp: &Viewport) -> Self {
        Self::new(vp.width as f32, vp.height as f32, vp.density)
    }

    /// dp → 物理像素（**不取整**；需要整数像素请用 [`round_dp`]）。
    pub fn dp(&self, dp: f32) -> f32 {
        dp * self.density
    }

    /// sp → 物理像素（含字体缩放，**不取整**）。
    pub fn sp(&self, sp: f32) -> f32 {
        sp * self.scaled_density
    }

    /// dp → 整数物理像素（Android `complexToDimensionPixelSize` 的四舍五入语义）。
    pub fn round_dp(&self, dp: f32) -> f32 {
        (dp * self.density).round()
    }

    /// sp → 整数物理像素。
    pub fn round_sp(&self, sp: f32) -> f32 {
        (sp * self.scaled_density).round()
    }

    /// 把任意浮点物理像素四舍五入到整数像素，保证几何与真机一致、渲染清晰。
    pub fn round_px(&self, px: f32) -> f32 {
        px.round()
    }

    /// 物理像素 → dp。
    pub fn px_to_dp(&self, px: f32) -> f32 {
        if self.density != 0.0 {
            px / self.density
        } else {
            0.0
        }
    }
}
