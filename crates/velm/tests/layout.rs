//! layout 模块契约测试（SPEC §7.4、§10.1）。
//!
//! 覆盖：MatchParent / Dp / WrapContent、纵 / 横排列、margin 偏移与尺寸扣减、
//! density 换算、根容器铺满、文本宽度 clamp、容器 WrapContent **按子节点求和**
//! （docs 缺陷回归）、嵌套树绝对坐标。

use velm::layout::{estimate_text_width, measure_and_layout};
use velm::view::{EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect, View};

type Msg = &'static str;

/// 浮点断言容差。
const EPS: f32 = 1e-4;

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < EPS,
        "{}: expected {}, got {}",
        what,
        expected,
        actual
    );
}

fn assert_rect(view: &View<Msg>, expected: Rect, what: &str) {
    let actual = rect_of(view);
    assert_close(actual.x, expected.x, &format!("{what} x"));
    assert_close(actual.y, expected.y, &format!("{what} y"));
    assert_close(actual.width, expected.width, &format!("{what} width"));
    assert_close(actual.height, expected.height, &format!("{what} height"));
}

fn rect_of(view: &View<Msg>) -> Rect {
    match view {
        View::TextView(tv) => tv.computed_rect,
        View::ViewGroup(vg) => vg.computed_rect,
        View::Widget(w) => w.common.computed_rect,
    }
}

fn children_of(view: &View<Msg>) -> &Vec<View<Msg>> {
    match view {
        View::ViewGroup(vg) => &vg.children,
        View::TextView(_) => panic!("expected ViewGroup"),
        View::Widget(_) => panic!("expected ViewGroup"),
    }
}

/// 固定尺寸的叶子节点（便于断言排列与 margin）。
fn fixed(w: f32, h: f32) -> View<Msg> {
    View::text_view("x").set_layout_params(LayoutParams {
        width: LayoutDimension::Dp(w),
        height: LayoutDimension::Dp(h),
        margin: EdgeInsets::default(),
    })
}

#[test]
fn root_match_parent_fills_whole_window() {
    let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    assert_rect(
        &root,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1080.0,
            height: 1920.0,
        },
        "root",
    );
}

#[test]
fn root_margin_is_subtracted_from_window_constraint() {
    let mut root =
        View::<Msg>::linear_layout(Orientation::Vertical, vec![]).set_layout_params(LayoutParams {
            width: LayoutDimension::MatchParent,
            height: LayoutDimension::MatchParent,
            margin: EdgeInsets::all(10.0),
        });
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    // 可用区 = 窗口 - 自身 margin，故不会溢出屏幕。
    assert_rect(
        &root,
        Rect {
            x: 10.0,
            y: 10.0,
            width: 1060.0,
            height: 1900.0,
        },
        "root with margin",
    );
}

#[test]
fn dp_multiplies_by_density() {
    let width_at = |density: f32| {
        let mut root = View::<Msg>::linear_layout(Orientation::Vertical, vec![]).set_layout_params(
            LayoutParams {
                width: LayoutDimension::Dp(80.0),
                height: LayoutDimension::Dp(50.0),
                margin: EdgeInsets::default(),
            },
        );
        measure_and_layout(&mut root, 1000.0, 1000.0, density);
        rect_of(&root)
    };

    assert_close(width_at(1.0).width, 80.0, "density 1.0 width");
    assert_close(width_at(1.0).height, 50.0, "density 1.0 height");
    assert_close(width_at(2.0).width, 160.0, "density 2.0 width");
    assert_close(width_at(2.0).height, 100.0, "density 2.0 height");
    assert_close(width_at(3.0).width, 240.0, "density 3.0 width");
}

#[test]
fn vertical_arrangement_stacks_children_on_y() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            fixed(1080.0, 100.0),
            fixed(1080.0, 100.0),
            fixed(1080.0, 100.0),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    let kids = children_of(&root);
    assert_rect(
        &kids[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1080.0,
            height: 100.0,
        },
        "child 0",
    );
    assert_rect(
        &kids[1],
        Rect {
            x: 0.0,
            y: 100.0,
            width: 1080.0,
            height: 100.0,
        },
        "child 1",
    );
    assert_rect(
        &kids[2],
        Rect {
            x: 0.0,
            y: 200.0,
            width: 1080.0,
            height: 100.0,
        },
        "child 2",
    );
}

#[test]
fn horizontal_arrangement_stacks_children_on_x() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Horizontal,
        vec![fixed(50.0, 50.0), fixed(50.0, 50.0)],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    let kids = children_of(&root);
    assert_rect(
        &kids[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        },
        "child 0",
    );
    assert_rect(
        &kids[1],
        Rect {
            x: 50.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        },
        "child 1",
    );
}

#[test]
fn margin_offsets_child_on_both_axes() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![fixed(50.0, 50.0).set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(50.0),
            height: LayoutDimension::Dp(50.0),
            margin: EdgeInsets {
                left: 10.0,
                top: 20.0,
                right: 0.0,
                bottom: 0.0,
            },
        })],
    );
    measure_and_layout(&mut root, 500.0, 500.0, 1.0);

    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 10.0,
            y: 20.0,
            width: 50.0,
            height: 50.0,
        },
        "child with margin",
    );
}

#[test]
fn margin_reduces_match_parent_size() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![View::text_view("x").set_layout_params(LayoutParams {
            width: LayoutDimension::MatchParent,
            height: LayoutDimension::Dp(10.0),
            margin: EdgeInsets {
                left: 10.0,
                top: 5.0,
                right: 20.0,
                bottom: 0.0,
            },
        })],
    );
    measure_and_layout(&mut root, 100.0, 200.0, 1.0);

    // 100 - 10 - 20 = 70
    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 10.0,
            y: 5.0,
            width: 70.0,
            height: 10.0,
        },
        "match_parent child with margin",
    );
}

#[test]
fn sibling_margins_separate_children() {
    let params = LayoutParams {
        width: LayoutDimension::Dp(50.0),
        height: LayoutDimension::Dp(50.0),
        margin: EdgeInsets::all(8.0),
    };
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            fixed(50.0, 50.0).set_layout_params(params),
            fixed(50.0, 50.0).set_layout_params(params),
        ],
    );
    measure_and_layout(&mut root, 500.0, 500.0, 1.0);

    let kids = children_of(&root);
    assert_close(rect_of(&kids[0]).y, 8.0, "child 0 y");
    // 推进量 = 50 + 8(上) + 8(下) = 66；第二个子节点再 +8 起。
    assert_close(rect_of(&kids[1]).y, 74.0, "child 1 y");
}

#[test]
fn text_view_wrap_content_uses_text_estimate() {
    let mut root = View::<Msg>::text_view("0").set_text_size(28.0);
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    // 宽 = 1 char * 28px * 0.6 = 16.8 → 四舍五入 17；高 = 28 * 1.4 = 39.2 → 39
    // （ADR-12：最终物理像素取整，与 Android complexToDimensionPixelSize 一致）。
    assert_close(rect_of(&root).width, 17.0, "wrap content width (rounded)");
    assert_close(rect_of(&root).height, 39.0, "wrap content height (rounded)");
}

#[test]
fn estimate_text_width_counts_chars() {
    assert_close(estimate_text_width("", 20.0), 0.0, "empty");
    assert_close(estimate_text_width("0", 20.0), 12.0, "one char");
    assert_close(estimate_text_width("0123456789", 20.0), 120.0, "ten chars");
    // 已知近似：中文按字符数估算，与真实字形宽度不同（P1 SC-13 替换为真实度量）。
    assert_close(estimate_text_width("你好", 20.0), 24.0, "two CJK chars");
}

#[test]
fn text_wrap_content_is_clamped_to_parent_width() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![View::text_view("0123456789").set_text_size(20.0)],
    );
    // 文本估算 120px > 父宽 100px → clamp 到 100，不溢出。
    measure_and_layout(&mut root, 100.0, 200.0, 1.0);

    assert_close(
        rect_of(&children_of(&root)[0]).width,
        100.0,
        "clamped width",
    );
}

#[test]
fn view_group_wrap_content_is_not_match_parent() {
    // docs 缺陷回归：容器 WrapContent 不得等于父尺寸，应按子节点求和。
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            View::<Msg>::linear_layout(
                Orientation::Horizontal,
                vec![fixed(50.0, 50.0), fixed(50.0, 50.0)],
            )
            .set_layout_params(LayoutParams {
                width: LayoutDimension::WrapContent,
                height: LayoutDimension::WrapContent,
                margin: EdgeInsets::default(),
            }),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
        "horizontal wrap_content group",
    );
}

#[test]
fn vertical_wrap_content_sums_heights_and_maxes_widths() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            View::<Msg>::linear_layout(
                Orientation::Vertical,
                vec![fixed(50.0, 30.0), fixed(80.0, 40.0)],
            )
            .set_layout_params(LayoutParams {
                width: LayoutDimension::WrapContent,
                height: LayoutDimension::WrapContent,
                margin: EdgeInsets::default(),
            }),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    // 纵向：主轴(高)求和 30+40=70，交叉轴(宽)取最大 80。
    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 70.0,
        },
        "vertical wrap_content group",
    );
}

#[test]
fn wrap_content_group_includes_child_margins() {
    let params = LayoutParams {
        width: LayoutDimension::Dp(50.0),
        height: LayoutDimension::Dp(50.0),
        margin: EdgeInsets::all(8.0),
    };
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            View::<Msg>::linear_layout(
                Orientation::Horizontal,
                vec![
                    fixed(50.0, 50.0).set_layout_params(params),
                    fixed(50.0, 50.0).set_layout_params(params),
                ],
            )
            .set_layout_params(LayoutParams {
                width: LayoutDimension::WrapContent,
                height: LayoutDimension::WrapContent,
                margin: EdgeInsets::default(),
            }),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    // 主轴推进量 = 50 + 8 + 8 = 66，两个子节点 → 132；交叉轴 = 66。
    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 132.0,
            height: 66.0,
        },
        "wrap_content group with child margins",
    );
}

#[test]
fn available_area_smaller_than_margin_clamps_to_zero() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![View::text_view("x").set_layout_params(LayoutParams {
            width: LayoutDimension::MatchParent,
            height: LayoutDimension::Dp(10.0),
            margin: EdgeInsets {
                left: 15.0,
                top: 0.0,
                right: 15.0,
                bottom: 0.0,
            },
        })],
    );
    // 父宽 20 < 左右 margin 合计 30 → 可用区退化为 0，不产生负尺寸。
    measure_and_layout(&mut root, 20.0, 200.0, 1.0);

    assert_close(
        rect_of(&children_of(&root)[0]).width,
        0.0,
        "clamped to zero",
    );
}

#[test]
fn nested_counter_layout_produces_absolute_pixel_coords() {
    // counter demo 骨架：纵向根 → 计数文本(28sp) + 横向按钮行(Dp 120x64, margin 8)。
    let button = |label: &str| {
        View::text_view(label).set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(120.0),
            height: LayoutDimension::Dp(64.0),
            margin: EdgeInsets::all(8.0),
        })
    };
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            View::text_view("0").set_text_size(28.0),
            View::<Msg>::linear_layout(Orientation::Horizontal, vec![button("+1"), button("-1")])
                .set_layout_params(LayoutParams {
                    width: LayoutDimension::WrapContent,
                    height: LayoutDimension::WrapContent,
                    margin: EdgeInsets::default(),
                }),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 1.0);

    let kids = children_of(&root);
    // 计数文本：WrapContent → 16.8 x 39.2，经像素取整为 17 x 39（ADR-12）。
    assert_rect(
        &kids[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 17.0,
            height: 39.0,
        },
        "count text",
    );
    // 按钮行：宽 2*(120+16)=272，高 64+16=80；游标推进到 39。
    assert_rect(
        &kids[1],
        Rect {
            x: 0.0,
            y: 39.0,
            width: 272.0,
            height: 80.0,
        },
        "button row",
    );

    let buttons = children_of(&kids[1]);
    assert_rect(
        &buttons[0],
        Rect {
            x: 8.0,
            y: 47.0,
            width: 120.0,
            height: 64.0,
        },
        "+1 button",
    );
    assert_rect(
        &buttons[1],
        Rect {
            x: 144.0,
            y: 47.0,
            width: 120.0,
            height: 64.0,
        },
        "-1 button",
    );
}

/// margin 声明在 dp 上，必须同 Dp/sp 一样乘 density（T7 端到端用例暴露的缺陷）。
///
/// density=1 时「漏乘」与「正确」结果一致，因此该回归必须在 density≠1 下断言。
#[test]
fn margin_is_scaled_by_density() {
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![fixed(100.0, 50.0).set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(100.0),
            height: LayoutDimension::Dp(50.0),
            margin: EdgeInsets::all(10.0),
        })],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    // 原点 = margin 10dp * 2 = 20px；尺寸 = 100dp * 2 = 200px / 50dp * 2 = 100px。
    assert_rect(
        &children_of(&root)[0],
        Rect {
            x: 20.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        },
        "child with dp margin at density=2",
    );
}

/// margin 参与主轴推进量时同样要用像素值：相邻兄弟间距 = (10+10)dp * 2 = 40px。
#[test]
fn sibling_margins_are_scaled_by_density() {
    let child = |margin: EdgeInsets| {
        View::text_view("x").set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(50.0),
            height: LayoutDimension::Dp(20.0),
            margin,
        })
    };
    let mut root = View::<Msg>::linear_layout(
        Orientation::Vertical,
        vec![
            child(EdgeInsets {
                top: 10.0,
                ..EdgeInsets::default()
            }),
            child(EdgeInsets {
                top: 10.0,
                ..EdgeInsets::default()
            }),
        ],
    );
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    let kids = children_of(&root);
    // 尺寸：50dp*2 = 100 宽，20dp*2 = 40 高。
    assert_close(rect_of(&kids[0]).height, 40.0, "child 0 height");
    // child 0: y = 10dp*2 = 20，占据 40 高 → 推进到 20+40+20(下 margin 0) = 60
    assert_close(rect_of(&kids[0]).y, 20.0, "child 0 y");
    // child 1: 再 +10dp*2 = 20 → y = 80
    assert_close(rect_of(&kids[1]).y, 80.0, "child 1 y");
}
