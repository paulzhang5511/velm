//! tests/render.rs — 绘制指令生成（SPEC §7.8 的纯逻辑部分，host 可测）。
//!
//! 覆盖：sp→px 换算、绘制顺序（画家算法）、零面积/空文本跳过、与
//! `measure_and_layout` 串联后的坐标一致性、基线居中几何。
//!
//! vello/wgpu 编码部分只能在设备侧验证（T13）。

use peniko::Color;

use velm::layout::measure_and_layout;
use velm::render::scene::{DrawCommand, build_draw_list, centered_baseline, sp_to_px};
use velm::view::View;
use velm::view::{EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect};

type Msg = u8;

const WHITE: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);
const GREEN: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);

/// 把节点的 `computed_rect` 直接写成给定值（绕过布局，专注绘制指令）。
fn with_rect(view: View<Msg>, rect: Rect) -> View<Msg> {
    match view {
        View::TextView(mut tv) => {
            tv.computed_rect = rect;
            View::TextView(tv)
        }
        View::ViewGroup(mut group) => {
            group.computed_rect = rect;
            View::ViewGroup(group)
        }
        View::Widget(mut w) => {
            w.common.computed_rect = rect;
            View::Widget(w)
        }
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// 取出背景指令的三元组 `(rect, rgba8, corner_radius)`。
fn as_fill(command: &DrawCommand) -> Option<(Rect, [u8; 4], f32)> {
    match command {
        DrawCommand::FillRect {
            rect,
            color,
            corner_radius,
        } => Some((*rect, color.to_rgba8().to_u8_array(), *corner_radius)),
        DrawCommand::Text { .. } => None,
        DrawCommand::StrokeRect { .. } => None,
    }
}

/// 取出文本指令的三元组 `(text, size_px, rgba8)`。
fn as_text(command: &DrawCommand) -> Option<(&str, f32, [u8; 4])> {
    match command {
        DrawCommand::Text {
            text,
            size_px,
            color,
            ..
        } => Some((text.as_str(), *size_px, color.to_rgba8().to_u8_array())),
        DrawCommand::FillRect { .. } => None,
        DrawCommand::StrokeRect { .. } => None,
    }
}

// ── 单位换算 ────────────────────────────────────────────────────────────────

#[test]
fn sp_to_px_multiplies_by_density() {
    assert_eq!(sp_to_px(16.0, 1.0), 16.0);
    assert_eq!(sp_to_px(16.0, 2.0), 32.0);
    assert_eq!(sp_to_px(28.0, 3.0), 84.0);
}

#[test]
fn sp_to_px_clamps_negative_density_to_zero() {
    // 密度异常（配置缺失/非法）时字号退化为 0，而不是负数——负字号会让
    // skrifa 的 Size 与 vello 的 font_size 语义失效。
    assert_eq!(sp_to_px(16.0, -2.0), 0.0);
}

// ── 指令生成 ────────────────────────────────────────────────────────────────

#[test]
fn text_without_background_yields_only_text() {
    let root = with_rect(View::text_view("count=0"), rect(10.0, 20.0, 100.0, 40.0));
    let commands = build_draw_list(&root, 2.0);

    assert_eq!(commands.len(), 1, "无背景时不产生填充指令");
    let (text, size_px, rgba) = as_text(&commands[0]).expect("期望文本指令");
    assert_eq!(text, "count=0");
    assert_eq!(size_px, 32.0, "16sp @ density=2 → 32px");
    assert_eq!(rgba, [0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn background_is_painted_before_text() {
    let root = with_rect(
        View::text_view("+1").set_background(GREEN, 12.0),
        rect(0.0, 0.0, 200.0, 100.0),
    );
    let commands = build_draw_list(&root, 1.0);

    assert_eq!(commands.len(), 2);
    let (r, rgba, radius) = as_fill(&commands[0]).expect("背景在前");
    assert_eq!(r, rect(0.0, 0.0, 200.0, 100.0));
    assert_eq!(rgba, [0x2E, 0x7D, 0x32, 0xFF]);
    assert_eq!(radius, 12.0);
    assert!(as_text(&commands[1]).is_some(), "文本在后");
}

#[test]
fn nested_tree_paints_parent_then_children_in_add_order() {
    let root = with_rect(
        View::linear_layout(
            Orientation::Vertical,
            vec![
                with_rect(View::text_view("first"), rect(0.0, 0.0, 50.0, 20.0)),
                with_rect(
                    View::text_view("second").set_background(GREEN, 0.0),
                    rect(0.0, 20.0, 50.0, 20.0),
                ),
            ],
        )
        .set_background(WHITE, 0.0),
        rect(0.0, 0.0, 100.0, 40.0),
    );
    let commands = build_draw_list(&root, 1.0);

    // 父背景 → 子 0 文本 → 子 1 背景 → 子 1 文本。
    assert_eq!(commands.len(), 4);
    assert!(as_fill(&commands[0]).is_some(), "父背景先画");
    assert_eq!(as_text(&commands[1]).expect("子 0 文本").0, "first");
    assert!(as_fill(&commands[2]).is_some(), "子 1 背景");
    assert_eq!(as_text(&commands[3]).expect("子 1 文本").0, "second");
}

#[test]
fn zero_area_subtree_is_skipped() {
    // 父容器零面积 → 子节点即便有尺寸也一并跳过（子节点受父约束必然不可见）。
    let root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![with_rect(
            View::text_view("hidden").set_background(GREEN, 0.0),
            rect(0.0, 0.0, 50.0, 20.0),
        )],
    );
    let commands = build_draw_list(&root, 1.0);
    assert!(
        commands.is_empty(),
        "未布局（全零 rect）的树不得产生绘制指令"
    );
}

#[test]
fn empty_text_and_missing_background_are_skipped() {
    let root = with_rect(View::text_view(""), rect(0.0, 0.0, 100.0, 40.0));
    assert!(
        build_draw_list(&root, 1.0).is_empty(),
        "空文本不产生绘制指令"
    );
}

// ── 与布局串联 ──────────────────────────────────────────────────────────────

#[test]
fn draw_list_rects_match_measured_rects() {
    let button = View::text_view("+1")
        .set_background(GREEN, 16.0)
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(200.0),
            height: LayoutDimension::Dp(100.0),
            margin: EdgeInsets::all(20.0),
        });
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    let commands = build_draw_list(&root, 2.0);
    assert_eq!(commands.len(), 2, "按钮背景 + 文本");

    // 20dp margin * 2 = 40px；200dp * 2 = 400px 宽、100dp * 2 = 200px 高。
    let (r, rgba, radius) = as_fill(&commands[0]).expect("按钮背景");
    assert_eq!(
        r,
        rect(40.0, 40.0, 400.0, 200.0),
        "绘制矩形必须与 computed_rect 完全一致"
    );
    assert_eq!(rgba, [0x2E, 0x7D, 0x32, 0xFF]);
    assert_eq!(radius, 16.0, "圆角是像素值，不随 density 缩放");

    let (_, size_px, _) = as_text(&commands[1]).expect("按钮文本");
    assert_eq!(size_px, 32.0, "16sp @ density=2 → 32px");
}

// ── 基线几何 ────────────────────────────────────────────────────────────────

#[test]
fn centered_baseline_places_text_block_in_the_middle() {
    // 高度 100，字块 ascent 30 + descent 10 = 40 → 上下各留 30 → 基线在 60。
    assert_eq!(centered_baseline(100.0, 30.0, 10.0), 60.0);
    // 字块恰好填满：基线 = ascent。
    assert_eq!(centered_baseline(40.0, 30.0, 10.0), 30.0);
    // 字块高于矩形：仍按公式居中（溢出，v1 不裁剪）。
    assert_eq!(centered_baseline(20.0, 30.0, 10.0), 20.0);
}
