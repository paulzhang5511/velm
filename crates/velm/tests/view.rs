//! view 模块契约测试（SPEC §7.2、§10.1）。
//!
//! 覆盖：默认值、链式设置（含背景色 / 圆角）、点击消息绑定、容器误用 no-op、
//! `Rect::contains` 闭区间语义、嵌套建树。

use peniko::Color;
use velm::view::{
    Background, DEFAULT_TEXT_COLOR, DEFAULT_TEXT_SIZE, EdgeInsets, LayoutDimension, LayoutParams,
    Orientation, Rect, View,
};

/// 测试中使用的消息类型（无需实现任何 trait 即可建树，SPEC §7.2 约束）。
type Msg = &'static str;

/// 取 TextView 的可变引用，非 TextView 则 panic（测试内部断言）。
fn as_text<Msg>(view: &View<Msg>) -> &velm::view::TextView<Msg> {
    match view {
        View::TextView(tv) => tv,
        View::ViewGroup(_) => panic!("expected TextView"),
    }
}

/// 取 ViewGroup 的可变引用，非 ViewGroup 则 panic（测试内部断言）。
fn as_group<Msg>(view: &View<Msg>) -> &velm::view::ViewGroup<Msg> {
    match view {
        View::ViewGroup(vg) => vg,
        View::TextView(_) => panic!("expected ViewGroup"),
    }
}

#[test]
fn layout_params_default_is_wrap_content_with_zero_margin() {
    let params = LayoutParams::default();

    assert_eq!(params.width, LayoutDimension::WrapContent);
    assert_eq!(params.height, LayoutDimension::WrapContent);
    assert_eq!(params.margin, EdgeInsets::default());
    assert_eq!(params.margin.left, 0.0);
    assert_eq!(params.margin.top, 0.0);
    assert_eq!(params.margin.right, 0.0);
    assert_eq!(params.margin.bottom, 0.0);
}

#[test]
fn edge_insets_all_sets_four_sides() {
    let inset = EdgeInsets::all(8.0);

    assert_eq!(inset.left, 8.0);
    assert_eq!(inset.top, 8.0);
    assert_eq!(inset.right, 8.0);
    assert_eq!(inset.bottom, 8.0);
}

#[test]
fn rect_contains_uses_closed_interval() {
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };

    // 内部命中。
    assert!(rect.contains(50.0, 50.0));
    // 四条边界均算命中（SPEC §7.5）。
    assert!(rect.contains(10.0, 50.0), "left edge");
    assert!(rect.contains(110.0, 50.0), "right edge");
    assert!(rect.contains(50.0, 20.0), "top edge");
    assert!(rect.contains(50.0, 70.0), "bottom edge");
    // 四角命中。
    assert!(rect.contains(10.0, 20.0));
    assert!(rect.contains(110.0, 70.0));
    // 外部不命中（含边界外 1 个浮点单位）。
    assert!(!rect.contains(9.9, 50.0));
    assert!(!rect.contains(110.1, 50.0));
    assert!(!rect.contains(50.0, 19.9));
    assert!(!rect.contains(50.0, 70.1));
}

#[test]
fn text_view_defaults_match_spec() {
    let view = View::<Msg>::text_view("count: 0");
    let tv = as_text(&view);

    assert_eq!(tv.text, "count: 0");
    assert_eq!(tv.text_size, DEFAULT_TEXT_SIZE);
    assert_eq!(tv.text_size, 16.0);
    assert_eq!(tv.text_color.to_rgba8(), DEFAULT_TEXT_COLOR.to_rgba8());
    // 默认文字为不透明白（ADR-10）。
    assert_eq!(tv.text_color.to_rgba8().r, 0xFF);
    assert_eq!(tv.text_color.to_rgba8().g, 0xFF);
    assert_eq!(tv.text_color.to_rgba8().b, 0xFF);
    assert_eq!(tv.text_color.to_rgba8().a, 0xFF);
    // 默认无背景、无点击消息、未布局。
    assert!(tv.background.color.is_none());
    assert_eq!(tv.background.corner_radius, 0.0);
    assert!(tv.on_click_listener.is_none());
    assert_eq!(tv.computed_rect, Rect::default());
    assert_eq!(tv.layout_params, LayoutParams::default());
}

#[test]
fn text_view_accepts_owned_and_borrowed_str() {
    let owned = View::<Msg>::text_view(String::from("owned"));
    let borrowed = View::<Msg>::text_view("borrowed");

    assert_eq!(as_text(&owned).text, "owned");
    assert_eq!(as_text(&borrowed).text, "borrowed");
}

#[test]
fn linear_layout_defaults_to_match_parent() {
    let view = View::<Msg>::linear_layout(Orientation::Vertical, vec![]);
    let vg = as_group(&view);

    assert_eq!(vg.orientation, Orientation::Vertical);
    assert_eq!(vg.layout_params.width, LayoutDimension::MatchParent);
    assert_eq!(vg.layout_params.height, LayoutDimension::MatchParent);
    assert_eq!(vg.layout_params.margin, EdgeInsets::default());
    assert!(vg.children.is_empty());
    assert!(vg.background.color.is_none());
    assert!(vg.on_click_listener.is_none());
    assert_eq!(vg.computed_rect, Rect::default());
}

#[test]
fn set_text_size_and_color_apply_to_text_view() {
    let red = Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF);
    let view = View::<Msg>::text_view("-1")
        .set_text_size(20.0)
        .set_text_color(red);
    let tv = as_text(&view);

    assert_eq!(tv.text_size, 20.0);
    assert_eq!(tv.text_color.to_rgba8(), red.to_rgba8());
}

#[test]
fn set_text_size_on_view_group_is_noop() {
    let view = View::<Msg>::linear_layout(
        Orientation::Horizontal,
        vec![View::text_view("+1"), View::text_view("-1")],
    )
    .set_text_size(99.0)
    .set_text_color(Color::from_rgba8(0x00, 0x00, 0x00, 0xFF));

    // no-op：仍是容器，子节点数量与内容不受影响，且不 panic。
    let vg = as_group(&view);
    assert_eq!(vg.children.len(), 2);
    assert_eq!(vg.orientation, Orientation::Horizontal);
    assert_eq!(as_text(&vg.children[0]).text, "+1");
    // 子节点字号仍是默认值，未被容器层的误用调用波及。
    assert_eq!(as_text(&vg.children[0]).text_size, DEFAULT_TEXT_SIZE);
}

#[test]
fn set_background_applies_to_text_view_and_view_group() {
    let green = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);

    let tv = View::<Msg>::text_view("+1").set_background(green, 12.0);
    let tv_bg = as_text(&tv).background;
    assert_eq!(tv_bg.color.map(|c| c.to_rgba8()), Some(green.to_rgba8()));
    assert_eq!(tv_bg.corner_radius, 12.0);

    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![]).set_background(green, 4.0);
    let vg_bg = as_group(&vg).background;
    assert_eq!(vg_bg.color.map(|c| c.to_rgba8()), Some(green.to_rgba8()));
    assert_eq!(vg_bg.corner_radius, 4.0);
}

#[test]
fn set_on_click_listener_binds_message_on_both_node_kinds() {
    let tv = View::<Msg>::text_view("+1").set_on_click_listener("increment");
    assert_eq!(as_text(&tv).on_click_listener, Some("increment"));

    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![])
        .set_on_click_listener("container");
    assert_eq!(as_group(&vg).on_click_listener, Some("container"));
}

#[test]
fn set_layout_params_overrides_width_height_and_margin() {
    let params = LayoutParams {
        width: LayoutDimension::Dp(120.0),
        height: LayoutDimension::WrapContent,
        margin: EdgeInsets::all(8.0),
    };

    let tv = View::<Msg>::text_view("+1").set_layout_params(params);
    assert_eq!(as_text(&tv).layout_params, params);

    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![]).set_layout_params(params);
    assert_eq!(as_group(&vg).layout_params, params);
}

#[test]
fn nested_tree_builds_with_chain_calls() {
    // counter demo 的树形骨架（ADR-10）：纵向根容器 → 计数文本 + 横向按钮行。
    let root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            View::text_view("0").set_text_size(28.0),
            View::linear_layout(
                Orientation::Horizontal,
                vec![
                    View::text_view("+1")
                        .set_background(Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF), 16.0)
                        .set_on_click_listener("increment")
                        .set_layout_params(LayoutParams {
                            width: LayoutDimension::Dp(120.0),
                            height: LayoutDimension::Dp(64.0),
                            margin: EdgeInsets::all(8.0),
                        }),
                    View::text_view("-1")
                        .set_background(Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF), 16.0)
                        .set_on_click_listener("decrement"),
                ],
            ),
        ],
    );

    let vg = as_group(&root);
    assert_eq!(vg.children.len(), 2);
    assert_eq!(as_text(&vg.children[0]).text_size, 28.0);

    let row = as_group(&vg.children[1]);
    assert_eq!(row.orientation, Orientation::Horizontal);
    assert_eq!(row.children.len(), 2);
    assert_eq!(
        as_text(&row.children[0]).layout_params.width,
        LayoutDimension::Dp(120.0)
    );
    assert_eq!(
        as_text(&row.children[0]).on_click_listener,
        Some("increment")
    );
    assert_eq!(
        as_text(&row.children[1]).on_click_listener,
        Some("decrement")
    );
    // 后添加者的背景色不被前序节点覆盖。
    assert_eq!(
        as_text(&row.children[1])
            .background
            .color
            .map(|c| c.to_rgba8()),
        Some(Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF).to_rgba8())
    );
}

#[test]
fn background_default_has_no_color_and_zero_radius() {
    let bg = Background::default();

    assert!(bg.color.is_none());
    assert_eq!(bg.corner_radius, 0.0);
}
