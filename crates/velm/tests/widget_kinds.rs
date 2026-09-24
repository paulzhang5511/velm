//! tests/widget_kinds.rs — 8 类复合组件的**全链路**覆盖（构造 / setter / 测量 / 绘制）。
//!
//! `tests/widget.rs` 覆盖按钮 / 进度条 / 卡片的主干；本文件补齐其余组件类别与全部
//! 链式 setter，以及 `render::scene` 中按 `WidgetKind` 派发的每条绘制分支——这些分支
//! 数量多、此前仅靠主干用例命中一部分，是覆盖率的主要缺口。

use peniko::Color;

use velm::layout::{measure_and_layout, measure_and_layout_with};
use velm::platform::DisplayMetrics;
use velm::render::scene::{DrawCommand, build_draw_list_with, centered_baseline, sp_to_px};
use velm::view::{
    EdgeInsets, LayoutDimension, LayoutParams, Orientation, ProgressOrientation, Rect, View,
    WidgetKind, WidgetView,
};

type Msg = &'static str;

const RED: Color = Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF);
const GREEN: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);
const BLUE: Color = Color::from_rgba8(0x21, 0x96, 0xF3, 0xFF);

/// 断言节点是 Widget 并借用。
fn widget<M>(view: &View<M>) -> &WidgetView<M> {
    match view {
        View::Widget(w) => w,
        _ => panic!("expected Widget"),
    }
}

/// 手动写入已布局矩形（绕过布局，专注绘制指令形态）。
fn set_rect(view: &mut View<Msg>, rect: Rect) {
    if let View::Widget(w) = view {
        w.common.computed_rect = rect;
    }
}

fn rect(w: f32, h: f32) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: w,
        height: h,
    }
}

// ── 构造器与 setter ───────────────────────────────────────────────────────────

#[test]
fn button_setters_apply_to_spec_and_common() {
    let v: View<Msg> = View::button("go")
        .set_text_size(18.0)
        .set_text_color(RED)
        .set_corner_radius_dp(6.0)
        .set_stroke(BLUE, 2.0)
        .set_padding(EdgeInsets::all(4.0))
        .set_background_color(GREEN);
    let w = widget(&v);
    assert_eq!(w.common.corner_radius_dp, 6.0);
    assert_eq!(w.common.padding, EdgeInsets::all(4.0));
    assert_eq!(w.common.stroke.width_dp, 2.0);
    assert_eq!(
        w.common.background.color.map(|c| c.to_rgba8()),
        Some(GREEN.to_rgba8())
    );
    match &w.kind {
        WidgetKind::Button(b) => {
            assert_eq!(b.text, "go");
            assert_eq!(b.text_size, 18.0);
            assert_eq!(b.text_color.to_rgba8(), RED.to_rgba8());
        }
        _ => panic!("期望 Button"),
    }
}

#[test]
fn card_with_orientation_builds_container() {
    let v: View<Msg> = View::card_with(Orientation::Horizontal, vec![View::text_view("c")])
        .set_corner_radius_dp(10.0)
        .set_padding(EdgeInsets::all(2.0));
    let w = widget(&v);
    assert_eq!(w.common.corner_radius_dp, 10.0);
    assert_eq!(w.common.padding, EdgeInsets::all(2.0));
    match &w.kind {
        WidgetKind::Card(c) => {
            assert_eq!(c.orientation, Orientation::Horizontal);
            assert_eq!(c.children.len(), 1);
            assert!(c.elevation_dp > 0.0, "默认阴影高度 > 0");
        }
        _ => panic!("期望 Card"),
    }
}

#[test]
fn image_placeholder_setter_applies() {
    let v: View<Msg> = View::image_view().set_image_placeholder(RED);
    match &widget(&v).kind {
        WidgetKind::Image(img) => assert_eq!(img.placeholder.to_rgba8(), RED.to_rgba8()),
        _ => panic!("期望 Image"),
    }
}

#[test]
fn progress_clamps_and_setter_applies() {
    // 越界进度被 clamp 到 [0,1]。
    let hi: View<Msg> = View::progress_bar().set_progress(9.0);
    match &widget(&hi).kind {
        WidgetKind::Progress(p) => assert_eq!(p.progress, 1.0),
        _ => panic!("期望 Progress"),
    }
    let lo: View<Msg> = View::progress_bar().set_progress(-3.0);
    match &widget(&lo).kind {
        WidgetKind::Progress(p) => assert_eq!(p.progress, 0.0),
        _ => panic!("期望 Progress"),
    }
}

#[test]
fn check_and_switch_toggle_setters() {
    let checked: View<Msg> = View::check_box(false).set_checked(true);
    match &widget(&checked).kind {
        WidgetKind::Check(c) => assert!(c.checked),
        _ => panic!("期望 Check"),
    }
    let on: View<Msg> = View::switch(false);
    match &widget(&on).kind {
        WidgetKind::Switch(s) => assert!(!s.checked),
        _ => panic!("期望 Switch"),
    }
}

#[test]
fn space_size_setter_applies() {
    let v: View<Msg> = View::space().set_space_size(10.0, 20.0);
    match &widget(&v).kind {
        WidgetKind::Space(s) => {
            assert_eq!(s.width_dp, 10.0);
            assert_eq!(s.height_dp, 20.0);
        }
        _ => panic!("期望 Space"),
    }
}

#[test]
fn edit_text_defaults_and_setters() {
    let v: View<Msg> = View::edit_text("请输入")
        .set_text_size(20.0)
        .set_text_color(BLUE);
    let w = widget(&v);
    // 输入框默认带 1dp 边框 + 12dp 内边距。
    assert_eq!(w.common.stroke.width_dp, 1.0);
    assert!(w.common.stroke.color.is_some());
    assert_eq!(w.common.padding, EdgeInsets::all(12.0));
    match &w.kind {
        WidgetKind::Edit(e) => {
            assert_eq!(e.hint, "请输入");
            assert!(e.text.is_empty());
            assert_eq!(e.text_size, 20.0);
            assert_eq!(e.text_color.to_rgba8(), BLUE.to_rgba8());
        }
        _ => panic!("期望 Edit"),
    }
}

#[test]
fn text_setters_are_noop_on_non_text_widgets() {
    // Image 不承载文本：set_text_size/color 命中 WidgetView 的 `_ => {}` 分支。
    let v: View<Msg> = View::image_view().set_text_size(99.0).set_text_color(RED);
    match &widget(&v).kind {
        WidgetKind::Image(img) => {
            // 占位色不受文字颜色影响。
            assert_eq!(
                img.placeholder.to_rgba8(),
                Color::from_rgba8(0x9E, 0x9E, 0x9E, 0xFF).to_rgba8()
            );
        }
        _ => panic!("期望 Image"),
    }
}

#[test]
fn text_setters_are_noop_on_view_group() {
    // ViewGroup 不承载文本：命中 `View::set_text_size/set_text_color` 的 no-op 分支。
    let v: View<Msg> = View::linear_layout(Orientation::Vertical, vec![]);
    let v = v.set_text_size(30.0).set_text_color(RED);
    assert!(matches!(v, View::ViewGroup(_)));
}

#[test]
fn stroke_padding_background_apply_to_all_three_node_kinds() {
    let cases: Vec<View<Msg>> = vec![
        View::text_view("t")
            .set_stroke(RED, 1.0)
            .set_padding(EdgeInsets::all(3.0)),
        View::linear_layout(Orientation::Vertical, vec![])
            .set_stroke(GREEN, 2.0)
            .set_padding(EdgeInsets::all(5.0)),
        View::button("b")
            .set_stroke(BLUE, 3.0)
            .set_padding(EdgeInsets::all(7.0)),
    ];
    // TextView 分支。
    match &cases[0] {
        View::TextView(tv) => {
            assert_eq!(tv.stroke.width_dp, 1.0);
            assert_eq!(tv.padding, EdgeInsets::all(3.0));
        }
        _ => panic!("期望 TextView"),
    }
    // ViewGroup 分支。
    match &cases[1] {
        View::ViewGroup(vg) => {
            assert_eq!(vg.stroke.width_dp, 2.0);
            assert_eq!(vg.padding, EdgeInsets::all(5.0));
        }
        _ => panic!("期望 ViewGroup"),
    }
    // Widget 分支。
    assert_eq!(widget(&cases[2]).common.stroke.width_dp, 3.0);

    // set_background 同样覆盖三种节点（Widget 走 common.background）。
    let tv = View::<Msg>::text_view("t").set_background(RED, 4.0);
    assert!(matches!(tv, View::TextView(_)));
    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![]).set_background(RED, 4.0);
    assert!(matches!(vg, View::ViewGroup(_)));
    let wd = View::<Msg>::button("b").set_background(RED, 4.0);
    assert_eq!(
        widget(&wd).common.background.corner_radius,
        4.0,
        "Widget 背景圆角以 px 写入"
    );
}

#[test]
fn listener_and_layout_params_apply_to_all_three_node_kinds() {
    // 覆盖 view/mod.rs 中 set_on_click_listener / set_layout_params 的三种节点分支。
    let tv = View::<Msg>::text_view("t")
        .set_on_click_listener("tv")
        .set_layout_params(View::<Msg>::dp_params(1.0, 2.0));
    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![])
        .set_on_click_listener("vg")
        .set_layout_params(View::<Msg>::dp_params(3.0, 4.0));
    let wd = View::<Msg>::button("b")
        .set_on_click_listener("wd")
        .set_layout_params(View::<Msg>::dp_params(5.0, 6.0));
    match &tv {
        View::TextView(t) => {
            assert_eq!(t.on_click_listener, Some("tv"));
            assert_eq!(t.layout_params.width, LayoutDimension::Dp(1.0));
        }
        _ => panic!("期望 TextView"),
    }
    match &vg {
        View::ViewGroup(g) => {
            assert!(g.on_click_listener.is_some());
            assert_eq!(g.layout_params.width, LayoutDimension::Dp(3.0));
        }
        _ => panic!("期望 ViewGroup"),
    }
    let w = widget(&wd);
    assert!(w.common.on_click_listener.is_some());
    assert_eq!(w.common.layout_params.width, LayoutDimension::Dp(5.0));
}

#[test]
fn dp_params_helper_builds_dp_layout() {
    let p = View::<Msg>::dp_params(200.0, 72.0);
    assert_eq!(p.width, LayoutDimension::Dp(200.0));
    assert_eq!(p.height, LayoutDimension::Dp(72.0));
    assert_eq!(p.margin, EdgeInsets::default());
}

// ── 测量：8 类组件全部走通 measure_widget ──────────────────────────────────────

#[test]
fn all_widget_kinds_measure_and_place() {
    let root_children: Vec<View<Msg>> = vec![
        View::button("b"),
        View::card(vec![View::text_view("x")]),
        View::image_view(),
        View::progress_bar(),
        View::check_box(false),
        View::switch(false),
        View::space(),
        View::edit_text("hint"),
    ];
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, root_children);
    // 用真正的 DisplayMetrics 入口（font_scale=1.0）。
    measure_and_layout_with(&mut root, &DisplayMetrics::new(1080.0, 1920.0, 2.0));

    let kids = match &root {
        View::ViewGroup(g) => &g.children,
        _ => panic!("root 应为容器"),
    };
    assert_eq!(kids.len(), 8);
    for (i, k) in kids.iter().enumerate() {
        let r = widget(k).common.computed_rect;
        assert!(r.width > 0.0, "组件 {i} 宽度应为正：{r:?}");
        assert!(r.height > 0.0, "组件 {i} 高度应为正：{r:?}");
    }
}

#[test]
fn card_child_is_placed_inside_content_box() {
    let card = View::card(vec![View::text_view("in").set_layout_params(
        LayoutParams {
            width: LayoutDimension::Dp(40.0),
            height: LayoutDimension::Dp(20.0),
            margin: EdgeInsets::default(),
        },
    )])
    .set_padding(EdgeInsets::all(10.0));
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![card]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    let kids = match &root {
        View::ViewGroup(g) => &g.children,
        _ => panic!("root 应为容器"),
    };
    let card_rect = widget(&kids[0]).common.computed_rect;
    // 10dp @ density=2 → 20px 内缩。
    match &widget(&kids[0]).kind {
        WidgetKind::Card(c) => match &c.children[0] {
            View::TextView(tv) => {
                assert!(
                    tv.computed_rect.x >= card_rect.x + 20.0 - 1.0,
                    "子节点受 padding 内缩"
                );
                assert!(
                    tv.computed_rect.y >= card_rect.y + 20.0 - 1.0,
                    "子节点受 padding 内缩"
                );
            }
            _ => panic!("子节点应为文本"),
        },
        _ => panic!("期望 Card"),
    }
}

// ── 绘制：按 WidgetKind 的每条 scene 分支 ─────────────────────────────────────

#[test]
fn zero_area_widget_draws_nothing() {
    let v: View<Msg> = View::button("b");
    // 未布局（rect 全零）→ 不产出任何指令。
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert!(cmds.is_empty());
}

#[test]
fn card_draws_shadow_background_and_children() {
    let mut v: View<Msg> = View::card(vec![View::text_view("c")]);
    set_rect(&mut v, rect(200.0, 100.0));
    if let View::Widget(w) = &mut v {
        match &mut w.kind {
            WidgetKind::Card(c) => {
                c.elevation_dp = 4.0;
                // 子节点补一个矩形，验证递归绘制。
                if let View::TextView(tv) = &mut c.children[0] {
                    tv.computed_rect = rect(50.0, 20.0);
                }
            }
            _ => panic!("期望 Card"),
        }
    }
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    // 阴影 + 卡片背景 + 子文本。
    assert!(cmds.len() >= 3, "阴影 + 背景 + 子节点，实得 {}", cmds.len());
    assert!(
        matches!(cmds[0], DrawCommand::FillRect { .. }),
        "阴影在下层"
    );
}

#[test]
fn card_without_elevation_skips_shadow() {
    let mut v: View<Msg> = View::card_with(Orientation::Vertical, vec![]);
    set_rect(&mut v, rect(100.0, 50.0));
    if let View::Widget(w) = &mut v {
        match &mut w.kind {
            // elevation_dp == 0 → 不画阴影（>0 才画）。
            WidgetKind::Card(c) => c.elevation_dp = 0.0,
            _ => panic!("期望 Card"),
        }
    }
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    // 仅背景（无阴影、无子节点）。
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawCommand::FillRect { .. }));
}

#[test]
fn image_draws_placeholder_and_optional_stroke() {
    let mut plain: View<Msg> = View::image_view();
    set_rect(&mut plain, rect(48.0, 48.0));
    let cmds = build_draw_list_with(&plain, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 1, "仅占位填充");
    assert!(matches!(cmds[0], DrawCommand::FillRect { .. }));

    let mut bordered: View<Msg> = View::image_view().set_stroke(RED, 2.0);
    set_rect(&mut bordered, rect(48.0, 48.0));
    let cmds = build_draw_list_with(&bordered, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 2, "占位 + 描边");
    assert!(matches!(cmds[1], DrawCommand::StrokeRect { .. }));
}

#[test]
fn progress_horizontal_zero_progress_only_track() {
    let mut v: View<Msg> = View::progress_bar(); // progress = 0
    set_rect(&mut v, rect(100.0, 8.0));
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 1, "进度 0 时只有轨道");
}

#[test]
fn progress_circular_track_radius_is_half_height() {
    let mut v: View<Msg> = View::progress_bar().set_progress(0.5);
    if let View::Widget(w) = &mut v {
        if let WidgetKind::Progress(p) = &mut w.kind {
            p.orientation = ProgressOrientation::Circular;
        }
        w.common.computed_rect = rect(48.0, 48.0);
    }
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 2, "轨道 + 环形填充");
    match (&cmds[0], &cmds[1]) {
        (
            DrawCommand::FillRect {
                corner_radius: tr, ..
            },
            DrawCommand::FillRect { rect: fill, .. },
        ) => {
            assert_eq!(*tr, 24.0, "环形轨道圆角 = 高度/2");
            assert_eq!(fill.width, 24.0, "50% → 直径 24");
        }
        _ => panic!("期望两条 FillRect"),
    }
}

#[test]
fn check_unchecked_draws_box_only_checked_marks() {
    let mut unchecked: View<Msg> = View::check_box(false);
    set_rect(&mut unchecked, rect(24.0, 24.0));
    let cmds = build_draw_list_with(&unchecked, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 1, "未勾选只有方框");
    assert!(matches!(cmds[0], DrawCommand::StrokeRect { .. }));

    let mut checked: View<Msg> = View::check_box(true);
    set_rect(&mut checked, rect(24.0, 24.0));
    let cmds = build_draw_list_with(&checked, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert_eq!(cmds.len(), 2, "勾选追加勾号文本");
    assert!(matches!(cmds[1], DrawCommand::Text { .. }));
}

#[test]
fn switch_thumb_moves_with_checked_state() {
    let mut off: View<Msg> = View::switch(false);
    set_rect(&mut off, rect(100.0, 50.0));
    let off_cmds = build_draw_list_with(&off, &DisplayMetrics::new(0.0, 0.0, 1.0));

    let mut on: View<Msg> = View::switch(true);
    set_rect(&mut on, rect(100.0, 50.0));
    let on_cmds = build_draw_list_with(&on, &DisplayMetrics::new(0.0, 0.0, 1.0));

    let thumb_x = |cmds: &Vec<DrawCommand>| match &cmds[1] {
        DrawCommand::FillRect { rect, .. } => rect.x,
        _ => panic!("第二条应为滑块"),
    };
    assert!(thumb_x(&on_cmds) > thumb_x(&off_cmds), "开启态滑块靠右");
}

#[test]
fn space_draws_nothing_but_occupies_size() {
    let mut v: View<Msg> = View::space();
    set_rect(&mut v, rect(16.0, 16.0));
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    assert!(cmds.is_empty(), "Space 不绘制内容");
}

#[test]
fn edit_shows_hint_when_empty_and_text_when_set() {
    let mut hint_only: View<Msg> = View::edit_text("提示");
    set_rect(&mut hint_only, rect(200.0, 48.0));
    let cmds = build_draw_list_with(&hint_only, &DisplayMetrics::new(0.0, 0.0, 2.0));
    let hint_text = cmds.iter().find_map(|c| match c {
        DrawCommand::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(hint_text.as_deref(), Some("提示"));

    // 手动塞入文本 → 显示文本而非 hint。
    let mut with_text: View<Msg> = View::edit_text("提示");
    if let View::Widget(w) = &mut with_text {
        if let WidgetKind::Edit(e) = &mut w.kind {
            e.text = "已输入".to_string();
        }
        w.common.computed_rect = rect(200.0, 48.0);
    }
    let cmds = build_draw_list_with(&with_text, &DisplayMetrics::new(0.0, 0.0, 2.0));
    let shown = cmds.iter().find_map(|c| match c {
        DrawCommand::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(shown.as_deref(), Some("已输入"));
}

#[test]
fn text_view_with_stroke_and_padding_draws_border_and_inset_text() {
    let mut v: View<Msg> = View::text_view("hi")
        .set_background(GREEN, 0.0)
        .set_stroke(RED, 2.0)
        .set_padding(EdgeInsets::all(5.0));
    if let View::TextView(tv) = &mut v {
        tv.computed_rect = rect(100.0, 40.0);
    }
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));
    // 背景 + 描边 + 文本。
    assert_eq!(cmds.len(), 3);
    assert!(matches!(cmds[1], DrawCommand::StrokeRect { .. }));
    if let DrawCommand::Text { rect, .. } = &cmds[2] {
        assert!(rect.x >= 10.0 - 1.0, "文本被 5dp*2 的内边距内缩");
    } else {
        panic!("期望文本指令");
    }
}

// ── 纯函数 ────────────────────────────────────────────────────────────────────

#[test]
fn sp_to_px_scales_and_clamps() {
    assert_eq!(sp_to_px(14.0, 2.0), 28.0);
    assert_eq!(sp_to_px(14.0, 0.0), 0.0);
    assert_eq!(sp_to_px(14.0, -3.0), 0.0, "负 scale 被夹到 0");
}

#[test]
fn centered_baseline_centers_glyph_box() {
    // height 20、ascent 12、descent 4 → 顶部偏移 = (20-16)/2 + 12 = 14。
    assert_eq!(centered_baseline(20.0, 12.0, 4.0), 14.0);
}
