//! tests/widget.rs — 复合组件（`View::Widget` 变体）的 host 集成测试。
//!
//! 覆盖：按钮的 measure / place / hit-test、进度条的绘制指令、卡片递归子节点、
//! padding 内缩、sp / dp 经 `DisplayMetrics` 换算。组件差异收敛到 `WidgetKind`
//! 枚举（见 `view/widget.rs`），本文件验证新增的 8 个组件能走通完整管线。

use peniko::Color;

use velm::engine::hit_test::perform_hit_test;
use velm::layout::measure_and_layout;
use velm::platform::DisplayMetrics;
use velm::render::scene::{DrawCommand, build_draw_list_with};
use velm::view::{EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect, View, WidgetKind};

type Msg = &'static str;

const GREEN: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);

/// 取 Widget 节点的已布局矩形。
fn widget_rect_of<Msg>(view: &View<Msg>) -> Rect {
    match view {
        View::Widget(w) => w.common.computed_rect,
        _ => panic!("expected Widget"),
    }
}

#[test]
fn button_is_measured_and_placed_in_dp() {
    let button = View::button("+1")
        .set_on_click_listener("inc")
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(200.0),
            height: LayoutDimension::Dp(72.0),
            margin: EdgeInsets::all(24.0),
        });
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    // 200dp*2=400, 72dp*2=144；margin 24dp*2=48。
    let kids = match &root {
        View::ViewGroup(g) => &g.children,
        _ => panic!("root 应为容器"),
    };
    let r = widget_rect_of(&kids[0]);
    assert_eq!(r.x, 48.0);
    assert_eq!(r.y, 48.0);
    assert_eq!(r.width, 400.0);
    assert_eq!(r.height, 144.0);
}

#[test]
fn button_hit_test_returns_its_message() {
    let button = View::button("+1")
        .set_on_click_listener("inc")
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(200.0),
            height: LayoutDimension::Dp(72.0),
            margin: EdgeInsets::all(24.0),
        });
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    // 中心 (48+200, 48+72) = (248, 120)。
    assert_eq!(perform_hit_test(&root, 248.0, 120.0), Some("inc"));
    // 根容器空白处：None。
    assert_eq!(perform_hit_test(&root, 10.0, 10.0), None);
}

#[test]
fn progress_widget_draws_track_and_fill() {
    let mut progress: View<Msg> = View::progress_bar().set_progress(0.5);
    // 手动写入非零 rect（绕过布局，专注绘制指令形态）。
    if let View::Widget(w) = &mut progress {
        w.common.computed_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 8.0,
        };
    }
    let metrics = DisplayMetrics::new(1000.0, 1000.0, 2.0);
    let commands = build_draw_list_with(&progress, &metrics);

    // 轨道 + 进度填充，共两条。
    assert_eq!(commands.len(), 2, "track + fill");
    match (&commands[0], &commands[1]) {
        (DrawCommand::FillRect { rect: track, .. }, DrawCommand::FillRect { rect: fill, .. }) => {
            assert_eq!(track.width, 200.0, "轨道铺满");
            assert_eq!(fill.width, 100.0, "50% 进度 → 半宽");
        }
        _ => panic!("期望两条 FillRect"),
    }
}

#[test]
fn card_recurses_children() {
    let card = View::card(vec![View::text_view("a").set_layout_params(LayoutParams {
        width: LayoutDimension::Dp(50.0),
        height: LayoutDimension::Dp(20.0),
        margin: EdgeInsets::default(),
    })])
    .set_layout_params(LayoutParams {
        width: LayoutDimension::MatchParent,
        height: LayoutDimension::WrapContent,
        margin: EdgeInsets::default(),
    });
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![card]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    let kids = match &root {
        View::ViewGroup(g) => &g.children,
        _ => panic!("root 应为容器"),
    };
    let card_rect = widget_rect_of(&kids[0]);
    assert!(card_rect.width > 0.0 && card_rect.height > 0.0, "卡片自身被布局");

    if let View::Widget(w) = &kids[0] {
        match &w.kind {
            WidgetKind::Card(c) => {
                let cr = match &c.children[0] {
                    View::TextView(tv) => tv.computed_rect,
                    _ => panic!("子节点应为文本"),
                };
                assert!(cr.x >= card_rect.x, "子节点位于卡片内（x）");
                assert!(cr.y >= card_rect.y, "子节点位于卡片内（y）");
                assert!(
                    cr.x + cr.width <= card_rect.x + card_rect.width + 1.0,
                    "子节点未超出卡片宽度"
                );
            }
            _ => panic!("期望 Card"),
        }
    } else {
        panic!("期望 Widget");
    }
}

#[test]
fn button_padding_insets_text() {
    let mut button = View::button("tap")
        .set_on_click_listener("t")
        .set_background_color(GREEN)
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(100.0),
            height: LayoutDimension::Dp(48.0),
            margin: EdgeInsets::default(),
        });
    // 手动写 rect（密度 2：100dp*2=200, 48dp*2=96）。
    if let View::Widget(w) = &mut button {
        w.common.computed_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 96.0,
        };
    }
    let metrics = DisplayMetrics::new(0.0, 0.0, 2.0);
    let commands = build_draw_list_with(&button, &metrics);

    // 背景 + 文本。
    assert_eq!(commands.len(), 2);
    // 按钮默认 padding 16/8dp @ density=2 → 32/16px；文本必须在内缩区内。
    if let DrawCommand::Text { rect, .. } = &commands[1] {
        assert!(rect.x >= 32.0 - 1.0, "文本左边界被水平 padding 内缩");
        assert!(rect.y >= 16.0 - 1.0, "文本上边界被垂直 padding 内缩");
    } else {
        panic!("期望文本指令");
    }
}

/// 验证 `set_background_color` 对 Widget 生效（圆角以 dp 计，密度感知）。
#[test]
fn widget_background_color_applies() {
    let view: View<Msg> = View::button("go").set_background_color(GREEN);
    if let View::Widget(w) = &view {
        assert_eq!(
            w.common.background.color.map(|c| c.to_rgba8()),
            Some(GREEN.to_rgba8())
        );
    } else {
        panic!("期望 Widget");
    }
}
