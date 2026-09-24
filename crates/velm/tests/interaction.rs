//! tests/interaction.rs — 交互态（`enabled` / `pressed`，ADR-14）的 host 测试。
//!
//! 覆盖三个层面：颜色变换（`Interaction::tint_fill` / `tint_content`）、命中测试
//! （禁用节点对点击透明、不向下传播）、渲染（禁用降透明、按下压暗），以及引擎
//! 用的按压态跟踪（`set_pressed_at` / `clear_pressed`）。

use peniko::Color;

use velm::engine::hit_test::{clear_pressed, perform_hit_test, set_pressed_at};
use velm::layout::measure_and_layout;
use velm::platform::DisplayMetrics;
use velm::render::scene::{DrawCommand, build_draw_list_with};
use velm::view::{EdgeInsets, Interaction, LayoutParams, Orientation, Rect, View};

type Msg = &'static str;

const GREEN: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF); // (46,125,50,255)
const RED: Color = Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF);

/// 取 Widget 节点已布局矩形。
fn widget_rect<M>(view: &View<M>) -> Rect {
    match view {
        View::Widget(w) => w.common.computed_rect,
        _ => panic!("expected Widget"),
    }
}

/// 取 Widget 的交互态。
fn widget_interaction<M>(view: &View<M>) -> Interaction {
    match view {
        View::Widget(w) => w.common.interaction,
        _ => panic!("expected Widget"),
    }
}

/// 手动写入 rect（绕过布局，专注绘制指令形态）。
fn set_rect(view: &mut View<Msg>, r: Rect) {
    if let View::Widget(w) = view {
        w.common.computed_rect = r;
    }
}

/// 带监听 + 固定 dp 尺寸的按钮。
fn sized_button(text: &'static str, msg: &'static str) -> View<Msg> {
    View::button(text)
        .set_on_click_listener(msg)
        .set_layout_params(LayoutParams {
            width: velm::view::LayoutDimension::Dp(200.0),
            height: velm::view::LayoutDimension::Dp(72.0),
            margin: EdgeInsets::all(8.0),
        })
}

/// 布局一棵只含单按钮的根容器，返回 root。
fn layout_button(button: View<Msg>) -> View<Msg> {
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);
    root
}

// ── 颜色变换（Interaction 本身） ───────────────────────────────────────────────

#[test]
fn normal_interaction_leaves_color_untouched() {
    let it = Interaction::default();
    assert!(it.is_normal());
    assert_eq!(it.tint_fill(GREEN).to_rgba8(), GREEN.to_rgba8());
    assert_eq!(it.tint_content(GREEN).to_rgba8(), GREEN.to_rgba8());
}

#[test]
fn disabled_halves_alpha_on_content_and_fill() {
    let it = Interaction {
        enabled: false,
        pressed: false,
    };
    assert!(!it.is_normal());
    // 内容（文字 / 描边）：alpha 255 → 128，RGB 不变。
    assert_eq!(
        it.tint_content(GREEN).to_rgba8().to_u8_array(),
        [46, 125, 50, 128]
    );
    // 填充同样降透明。
    assert_eq!(
        it.tint_fill(GREEN).to_rgba8().to_u8_array(),
        [46, 125, 50, 128]
    );
}

#[test]
fn pressed_darkens_fill_but_not_content() {
    let it = Interaction {
        enabled: true,
        pressed: true,
    };
    // 填充：RGB 乘 0.85 压暗，alpha 不变。
    assert_eq!(
        it.tint_fill(GREEN).to_rgba8().to_u8_array(),
        [39, 106, 43, 255]
    );
    // 内容：按下不改色。
    assert_eq!(it.tint_content(GREEN).to_rgba8(), GREEN.to_rgba8());
}

#[test]
fn disabled_takes_precedence_over_pressed() {
    let it = Interaction {
        enabled: false,
        pressed: true,
    };
    // 禁用 + 按下 → 只降透明、不压暗（enabled 优先）。
    assert_eq!(
        it.tint_fill(GREEN).to_rgba8().to_u8_array(),
        [46, 125, 50, 128]
    );
}

// ── set_enabled / set_pressed 三种节点 ────────────────────────────────────────

#[test]
fn enabled_and_pressed_apply_to_all_three_node_kinds() {
    let tv = View::<Msg>::text_view("t")
        .set_enabled(false)
        .set_pressed(true);
    match &tv {
        View::TextView(t) => {
            assert!(!t.interaction.enabled);
            assert!(t.interaction.pressed);
        }
        _ => panic!("期望 TextView"),
    }

    let vg = View::<Msg>::linear_layout(Orientation::Vertical, vec![])
        .set_enabled(false)
        .set_pressed(true);
    match &vg {
        View::ViewGroup(g) => {
            assert!(!g.interaction.enabled);
            assert!(g.interaction.pressed);
        }
        _ => panic!("期望 ViewGroup"),
    }

    let wd = View::<Msg>::button("b")
        .set_enabled(false)
        .set_pressed(true);
    let it = widget_interaction(&wd);
    assert!(!it.enabled);
    assert!(it.pressed);
}

// ── 命中测试：禁用透明、不向下传播 ────────────────────────────────────────────

#[test]
fn disabled_node_is_transparent_to_click() {
    // 同一个按钮，启用时命中、禁用时不命中。
    let enabled_root = layout_button(sized_button("b", "hit"));
    let r = widget_rect(match &enabled_root {
        View::ViewGroup(g) => &g.children[0],
        _ => panic!(),
    });
    let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
    assert_eq!(perform_hit_test(&enabled_root, cx, cy), Some("hit"));

    let disabled_root = layout_button(sized_button("b", "hit").set_enabled(false));
    assert_eq!(
        perform_hit_test(&disabled_root, cx, cy),
        None,
        "禁用节点不响应点击"
    );
}

#[test]
fn disabled_container_does_not_disable_children() {
    let child = sized_button("c", "child");
    let group = View::<Msg>::linear_layout(Orientation::Vertical, vec![child])
        .set_on_click_listener("group")
        .set_enabled(false);
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![group]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    let group_rect = match &root {
        View::ViewGroup(g) => match &g.children[0] {
            View::ViewGroup(inner) => inner.computed_rect,
            _ => panic!("子节点应为容器"),
        },
        _ => panic!("root 应为容器"),
    };
    let child_rect = match &root {
        View::ViewGroup(g) => match &g.children[0] {
            View::ViewGroup(inner) => match &inner.children[0] {
                View::Widget(w) => w.common.computed_rect,
                _ => panic!("应为按钮"),
            },
            _ => panic!(),
        },
        _ => panic!(),
    };

    // 子节点（启用）仍可点击——禁用只影响本节点，不向下传播。
    assert_eq!(
        perform_hit_test(
            &root,
            child_rect.x + child_rect.width / 2.0,
            child_rect.y + child_rect.height / 2.0
        ),
        Some("child")
    );
    // 容器自身的监听因禁用而失效：点击容器内、子节点外的位置不产生消息。
    let empty_y = group_rect.y + group_rect.height - 2.0;
    assert_eq!(perform_hit_test(&root, group_rect.x + 4.0, empty_y), None);
}

// ── 渲染：禁用降透明 / 按下压暗 ───────────────────────────────────────────────

#[test]
fn disabled_button_fill_and_text_are_dimmed() {
    let mut v: View<Msg> = View::button("go")
        .set_background_color(GREEN)
        .set_enabled(false);
    set_rect(
        &mut v,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 96.0,
        },
    );
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));

    // 背景填充 + 文本。
    match &cmds[0] {
        DrawCommand::FillRect { color, .. } => {
            assert_eq!(
                color.to_rgba8().to_u8_array(),
                [46, 125, 50, 128],
                "背景降透明"
            );
        }
        _ => panic!("首条应为背景"),
    }
    match &cmds[1] {
        DrawCommand::Text { color, .. } => {
            assert_eq!(color.to_rgba8().to_u8_array()[3], 128, "文字降透明");
        }
        _ => panic!("次条应为文本"),
    }
}

#[test]
fn pressed_button_darkens_background_but_keeps_text() {
    let mut v: View<Msg> = View::button("go")
        .set_background_color(GREEN)
        .set_text_color(RED)
        .set_pressed(true);
    set_rect(
        &mut v,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 96.0,
        },
    );
    let cmds = build_draw_list_with(&v, &DisplayMetrics::new(0.0, 0.0, 2.0));

    match &cmds[0] {
        DrawCommand::FillRect { color, .. } => {
            assert_eq!(
                color.to_rgba8().to_u8_array(),
                [39, 106, 43, 255],
                "背景压暗"
            );
        }
        _ => panic!("首条应为背景"),
    }
    match &cmds[1] {
        DrawCommand::Text { color, .. } => {
            assert_eq!(color.to_rgba8(), RED.to_rgba8(), "文字不因按下变色");
        }
        _ => panic!("次条应为文本"),
    }
}

// ── 引擎用：按压态跟踪 ────────────────────────────────────────────────────────

#[test]
fn set_pressed_at_marks_hit_node_and_reports_change() {
    let mut root = layout_button(sized_button("b", "hit"));
    let r = match &root {
        View::ViewGroup(g) => widget_rect(&g.children[0]),
        _ => panic!(),
    };
    let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);

    assert!(
        set_pressed_at(&mut root, cx, cy, true),
        "首次置位应报告变化"
    );
    match &root {
        View::ViewGroup(g) => assert!(widget_interaction(&g.children[0]).pressed),
        _ => panic!(),
    }
    // 重复置位无变化。
    assert!(
        !set_pressed_at(&mut root, cx, cy, true),
        "重复置位不应报告变化"
    );

    // 清除。
    assert!(clear_pressed(&mut root));
    match &root {
        View::ViewGroup(g) => assert!(!widget_interaction(&g.children[0]).pressed),
        _ => panic!(),
    }
    assert!(!clear_pressed(&mut root), "已清除后不应报告变化");
}

#[test]
fn set_pressed_at_ignores_disabled_node() {
    let mut root = layout_button(sized_button("b", "hit").set_enabled(false));
    let r = match &root {
        View::ViewGroup(g) => widget_rect(&g.children[0]),
        _ => panic!(),
    };
    // 禁用节点不获得按压反馈。
    assert!(!set_pressed_at(
        &mut root,
        r.x + r.width / 2.0,
        r.y + r.height / 2.0,
        true
    ));
    match &root {
        View::ViewGroup(g) => assert!(!widget_interaction(&g.children[0]).pressed),
        _ => panic!(),
    }
}

#[test]
fn set_pressed_at_misses_on_blank_area() {
    let mut root = layout_button(sized_button("b", "hit"));
    // 按钮带 8dp margin，左上角空白处不应命中。
    assert!(!set_pressed_at(&mut root, 1.0, 1.0, true));
}
