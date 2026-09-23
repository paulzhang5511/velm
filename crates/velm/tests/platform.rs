//! tests/platform.rs — 屏幕配置与 density 换算（SPEC §7.1、ADR-12）。
//!
//! `NativeWindowWrapper` 属 android-only FFI，只能在真机验证（T13）；这里只
//! 覆盖它依赖的纯换算逻辑，并串一遍「density → 布局」的实际效果。

use velm::layout::measure_and_layout;
use velm::platform::{DENSITY_MEDIUM_DPI, ScreenConfig, density_from_dpi};
use velm::view::{LayoutDimension, LayoutParams, Orientation, Rect, View};

type Msg = &'static str;

fn rect_of(view: &View<Msg>) -> Rect {
    match view {
        View::TextView(tv) => tv.computed_rect,
        View::ViewGroup(vg) => vg.computed_rect,
        View::Widget(w) => w.common.computed_rect,
    }
}

#[test]
fn dpi_is_divided_by_160() {
    assert_eq!(density_from_dpi(160), 1.0);
    assert_eq!(density_from_dpi(320), 2.0);
    assert_eq!(density_from_dpi(480), 3.0);
    assert_eq!(density_from_dpi(440), 2.75);
}

#[test]
fn medium_dpi_constant_is_160() {
    assert_eq!(DENSITY_MEDIUM_DPI, 160);
    assert_eq!(density_from_dpi(DENSITY_MEDIUM_DPI), 1.0);
}

#[test]
fn wildcard_and_invalid_dpi_fall_back_to_one() {
    // ACONFIGURATION_DENSITY_ANY / NONE 是通配语义，不是真实 dpi。
    assert_eq!(density_from_dpi(65534), 1.0, "DENSITY_ANY 必须兜底");
    assert_eq!(density_from_dpi(65535), 1.0, "DENSITY_NONE 必须兜底");
    assert_eq!(density_from_dpi(0), 1.0);
    assert_eq!(density_from_dpi(-1), 1.0);
}

#[test]
fn screen_config_carries_size_and_density() {
    let config = ScreenConfig {
        width: 1080,
        height: 1920,
        density: density_from_dpi(320),
    };
    assert_eq!(config.density, 2.0);
    let copied = config;
    assert_eq!(copied, config);
    assert!(format!("{config:?}").contains("density: 2.0"));
}

#[test]
fn density_from_screen_config_reaches_layout() {
    // 端到端：xxhdpi(480) 设备上 100dp 的按钮必须是 300 物理像素宽。
    let config = ScreenConfig {
        width: 1080,
        height: 1920,
        density: density_from_dpi(480),
    };
    let button = View::text_view("+1").set_layout_params(LayoutParams {
        width: LayoutDimension::Dp(100.0),
        height: LayoutDimension::Dp(50.0),
        margin: Default::default(),
    });
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(
        &mut root,
        config.width as f32,
        config.height as f32,
        config.density,
    );

    let kids = match &root {
        View::ViewGroup(vg) => &vg.children,
        View::TextView(_) => panic!("期望 ViewGroup"),
        View::Widget(_) => panic!("期望 ViewGroup"),
    };
    assert_eq!(rect_of(&kids[0]).width, 300.0, "100dp @ density=3 → 300px");
    assert_eq!(rect_of(&kids[0]).height, 150.0, "50dp @ density=3 → 150px");
}
