//! tests/display_metrics.rs — Android 风格密度模型（SPEC §7.1、ADR-12）。
//!
//! 覆盖：dp/sp 分离、字体缩放、像素取整、密度桶选取、dpi 换算、反算 px→dp。
//! 这些量全部经由 [`DisplayMetrics`]，禁止在布局 / 渲染里散落 `value * 2.0`。

use velm::platform::{DensityBucket, DisplayMetrics};

#[test]
fn dp_multiplies_by_density_only() {
    let m = DisplayMetrics::new(1080.0, 1920.0, 2.0);
    assert_eq!(m.dp(100.0), 200.0);
    assert_eq!(m.dp(0.0), 0.0);
}

#[test]
fn sp_includes_font_scale() {
    // density=2 但字体放大 1.5 倍 → sp 比 dp 多乘 font_scale。
    let m = DisplayMetrics::with_font_scale(1080.0, 1920.0, 2.0, 1.5);
    assert_eq!(
        m.scaled_density, 3.0,
        "scaled_density = density * font_scale"
    );
    assert_eq!(m.dp(100.0), 200.0, "dp 不受字体缩放影响");
    assert_eq!(m.sp(100.0), 300.0, "sp 含字体缩放");
}

#[test]
fn font_scale_does_not_change_dp() {
    let base = DisplayMetrics::new(1080.0, 1920.0, 3.0);
    let scaled = DisplayMetrics::with_font_scale(1080.0, 1920.0, 3.0, 2.0);
    assert_eq!(base.dp(50.0), scaled.dp(50.0), "dp 与 font_scale 无关");
    assert_ne!(base.sp(50.0), scaled.sp(50.0), "sp 随 font_scale 变化");
}

#[test]
fn round_dp_rounds_to_nearest_pixel() {
    let m = DisplayMetrics::new(1080.0, 1920.0, 2.0);
    assert_eq!(m.round_dp(10.0), 20.0);
    // 1dp @ density=2 = 2px 整；用非常规值验证四舍五入。
    assert_eq!(m.round_dp(10.5), 21.0);
    assert_eq!(m.round_dp(10.4), 21.0, "四舍五入进位");
    assert_eq!(m.round_dp(10.1), 20.0, "四舍五入舍去");
}

#[test]
fn round_sp_rounds_to_integer_pixels() {
    let m = DisplayMetrics::with_font_scale(1080.0, 1920.0, 2.0, 1.5);
    // 16sp → 16 * 3.0 = 48.0
    assert_eq!(m.round_sp(16.0), 48.0);
    // 14sp → 42.0
    assert_eq!(m.round_sp(14.0), 42.0);
}

#[test]
fn round_px_rounds_arbitrary_pixels() {
    let m = DisplayMetrics::new(1080.0, 1920.0, 2.0);
    assert_eq!(m.round_px(10.4), 10.0);
    assert_eq!(m.round_px(10.6), 11.0);
}

#[test]
fn density_bucket_selection() {
    assert_eq!(DensityBucket::from_density(0.75), DensityBucket::Ldpi);
    assert_eq!(DensityBucket::from_density(1.0), DensityBucket::Mdpi);
    assert_eq!(DensityBucket::from_density(1.5), DensityBucket::Hdpi);
    assert_eq!(DensityBucket::from_density(2.0), DensityBucket::Xhdpi);
    assert_eq!(DensityBucket::from_density(3.0), DensityBucket::Xxhdpi);
    assert_eq!(DensityBucket::from_density(4.0), DensityBucket::Xxxhdpi);
    // 非标准密度落入 Other 桶并保留原始 scale。
    assert_eq!(
        DensityBucket::from_density(2.75),
        DensityBucket::Other(2.75)
    );
}

#[test]
fn bucket_scale_matches_density() {
    assert_eq!(DensityBucket::Ldpi.scale(), 0.75);
    assert_eq!(DensityBucket::Mdpi.scale(), 1.0);
    assert_eq!(DensityBucket::Hdpi.scale(), 1.5);
    assert_eq!(DensityBucket::Xhdpi.scale(), 2.0);
    assert_eq!(DensityBucket::Xxhdpi.scale(), 3.0);
    assert_eq!(DensityBucket::Xxxhdpi.scale(), 4.0);
}

#[test]
fn non_positive_density_falls_back_to_mdpi() {
    // 配置缺失/非法时退化为基准密度，避免负密度污染几何。
    let m = DisplayMetrics::new(1080.0, 1920.0, 0.0);
    assert_eq!(m.density, 1.0);
    assert_eq!(m.bucket, DensityBucket::Mdpi);
}

#[test]
fn from_dpi_matches_android_division() {
    // 480dpi → density 3.0（480/160）。
    let m = DisplayMetrics::from_dpi(1080.0, 1920.0, 480, 1.0);
    assert_eq!(m.density, 3.0);
    assert_eq!(m.bucket, DensityBucket::Xxhdpi);
}

#[test]
fn px_to_dp_inverts_dp() {
    let m = DisplayMetrics::new(1080.0, 1920.0, 2.0);
    assert_eq!(m.px_to_dp(200.0), 100.0);
    assert_eq!(m.px_to_dp(0.0), 0.0);
}

#[test]
fn padding_is_applied_in_dp_through_metrics() {
    // padding 在布局阶段乘 density；这里验证同一套换算口径一致（8dp @ density=3）。
    let m = DisplayMetrics::new(1080.0, 1920.0, 3.0);
    assert_eq!(m.round_dp(8.0), 24.0, "8dp @ density=3 → 24px");
}
