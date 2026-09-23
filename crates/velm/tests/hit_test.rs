//! tests/hit_test.rs — engine::hit_test 的 host 单测（SPEC §7.5）。
//!
//! 覆盖：叶子命中/未命中、无监听节点、重叠时后添加者优先、透明容器穿透、
//! 容器回落、子节点优先于容器、闭区间边界、父容器剪枝、深层嵌套、树不被
//! 修改，以及与 `measure_and_layout` 的端到端串联。

use velm::engine::hit_test::perform_hit_test;
use velm::layout::measure_and_layout;
use velm::view::{LayoutDimension, LayoutParams, Orientation, Rect, View};

/// 测试用消息：可比较，便于断言命中结果。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Root,
    First,
    Second,
    Container,
    Inner,
    Deep,
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// 直接写入布局结果矩形，绕过 measure（本文件多数用例只关心命中判定本身）。
fn with_rect(mut view: View<Msg>, r: Rect) -> View<Msg> {
    match &mut view {
        View::TextView(tv) => tv.computed_rect = r,
        View::ViewGroup(vg) => vg.computed_rect = r,
        View::Widget(w) => w.common.computed_rect = r,
    }
    view
}

fn leaf(msg: Option<Msg>, r: Rect) -> View<Msg> {
    let view = View::text_view("x");
    let view = match msg {
        Some(m) => view.set_on_click_listener(m),
        None => view,
    };
    with_rect(view, r)
}

fn group(rect: Rect, msg: Option<Msg>, children: Vec<View<Msg>>) -> View<Msg> {
    let mut view = View::linear_layout(Orientation::Vertical, children);
    if let Some(m) = msg {
        view = view.set_on_click_listener(m);
    }
    with_rect(view, rect)
}

// ── 叶子节点 ────────────────────────────────────────────────────────────────

#[test]
fn leaf_hit_returns_its_message() {
    let root = leaf(Some(Msg::First), rect(0.0, 0.0, 100.0, 50.0));
    assert_eq!(perform_hit_test(&root, 50.0, 25.0), Some(Msg::First));
}

#[test]
fn leaf_miss_returns_none() {
    let root = leaf(Some(Msg::First), rect(0.0, 0.0, 100.0, 50.0));
    assert_eq!(perform_hit_test(&root, 101.0, 25.0), None);
    assert_eq!(perform_hit_test(&root, 50.0, 51.0), None);
    assert_eq!(perform_hit_test(&root, -1.0, 25.0), None);
}

#[test]
fn leaf_without_listener_is_transparent() {
    let root = leaf(None, rect(0.0, 0.0, 100.0, 50.0));
    assert_eq!(perform_hit_test(&root, 50.0, 25.0), None);
}

// ── 闭区间边界 ──────────────────────────────────────────────────────────────

#[test]
fn boundary_points_are_inside() {
    let root = leaf(Some(Msg::First), rect(10.0, 20.0, 100.0, 50.0));
    // 四角与四边落在闭区间内均算命中。
    for (x, y) in [
        (10.0, 20.0),
        (110.0, 20.0),
        (10.0, 70.0),
        (110.0, 70.0),
        (60.0, 20.0),
        (60.0, 70.0),
    ] {
        assert_eq!(
            perform_hit_test(&root, x, y),
            Some(Msg::First),
            "boundary ({x}, {y}) should hit"
        );
    }
    // 越界一位即不命中。
    assert_eq!(perform_hit_test(&root, 9.999, 20.0), None);
    assert_eq!(perform_hit_test(&root, 10.0, 19.999), None);
    assert_eq!(perform_hit_test(&root, 110.001, 70.0), None);
    assert_eq!(perform_hit_test(&root, 110.0, 70.001), None);
}

// ── 子节点逆序优先 / 容器回落 ───────────────────────────────────────────────

#[test]
fn overlapping_children_later_added_wins() {
    // 两个子节点完全重叠，后添加的在上层，应优先响应。
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        None,
        vec![
            leaf(Some(Msg::First), rect(0.0, 0.0, 100.0, 100.0)),
            leaf(Some(Msg::Second), rect(0.0, 0.0, 100.0, 100.0)),
        ],
    );
    assert_eq!(perform_hit_test(&root, 50.0, 50.0), Some(Msg::Second));
}

#[test]
fn child_wins_over_container_listener() {
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        Some(Msg::Container),
        vec![leaf(Some(Msg::First), rect(0.0, 0.0, 50.0, 100.0))],
    );
    // 落在子节点上：子节点优先。
    assert_eq!(perform_hit_test(&root, 25.0, 50.0), Some(Msg::First));
    // 落在容器空白处：回落到容器自身监听。
    assert_eq!(perform_hit_test(&root, 75.0, 50.0), Some(Msg::Container));
}

#[test]
fn container_fallback_used_when_children_miss() {
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        Some(Msg::Container),
        vec![leaf(Some(Msg::First), rect(0.0, 0.0, 10.0, 10.0))],
    );
    assert_eq!(perform_hit_test(&root, 90.0, 90.0), Some(Msg::Container));
}

#[test]
fn empty_container_falls_back_to_itself() {
    let root = group(rect(0.0, 0.0, 100.0, 100.0), Some(Msg::Container), vec![]);
    assert_eq!(perform_hit_test(&root, 50.0, 50.0), Some(Msg::Container));
}

#[test]
fn container_without_listener_and_missed_children_returns_none() {
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        None,
        vec![leaf(Some(Msg::First), rect(0.0, 0.0, 10.0, 10.0))],
    );
    assert_eq!(perform_hit_test(&root, 90.0, 90.0), None);
}

// ── 透明容器穿透（T7 定稿语义） ─────────────────────────────────────────────

#[test]
fn transparent_container_passes_through_to_lower_sibling() {
    // 上层是一个完全覆盖、但自身与后代都不产生消息的容器；
    // 点击应穿透到它下面那个兄弟节点。
    let overlay = group(
        rect(0.0, 0.0, 100.0, 100.0),
        None,
        vec![leaf(None, rect(0.0, 0.0, 100.0, 100.0))],
    );
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        None,
        vec![
            leaf(Some(Msg::First), rect(0.0, 0.0, 100.0, 100.0)),
            overlay,
        ],
    );
    assert_eq!(perform_hit_test(&root, 50.0, 50.0), Some(Msg::First));
}

#[test]
fn container_with_listener_blocks_lower_sibling() {
    // 一旦上层容器绑了监听，它就不再是透明的，点击被它拦下。
    let overlay = group(rect(0.0, 0.0, 100.0, 100.0), Some(Msg::Container), vec![]);
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        None,
        vec![
            leaf(Some(Msg::First), rect(0.0, 0.0, 100.0, 100.0)),
            overlay,
        ],
    );
    assert_eq!(perform_hit_test(&root, 50.0, 50.0), Some(Msg::Container));
}

// ── 父容器剪枝 ──────────────────────────────────────────────────────────────

#[test]
fn parent_rect_excludes_whole_subtree() {
    // 子节点 rect 故意超出父容器：父不包含时整棵子树被排除。
    let root = group(
        rect(0.0, 0.0, 50.0, 50.0),
        None,
        vec![leaf(Some(Msg::First), rect(0.0, 0.0, 500.0, 500.0))],
    );
    assert_eq!(perform_hit_test(&root, 80.0, 80.0), None);
}

// ── 深层嵌套 ────────────────────────────────────────────────────────────────

#[test]
fn deep_nesting_hits_innermost_node() {
    let inner = group(
        rect(20.0, 20.0, 40.0, 40.0),
        Some(Msg::Container),
        vec![leaf(Some(Msg::Deep), rect(20.0, 20.0, 10.0, 10.0))],
    );
    let middle = group(rect(10.0, 10.0, 80.0, 80.0), Some(Msg::Inner), vec![inner]);
    let root = group(rect(0.0, 0.0, 100.0, 100.0), Some(Msg::Root), vec![middle]);

    assert_eq!(perform_hit_test(&root, 25.0, 25.0), Some(Msg::Deep));
    assert_eq!(perform_hit_test(&root, 50.0, 50.0), Some(Msg::Container));
    assert_eq!(perform_hit_test(&root, 90.0, 90.0), Some(Msg::Inner));
    assert_eq!(perform_hit_test(&root, 99.0, 99.0), Some(Msg::Root));
}

// ── 不修改视图树 ────────────────────────────────────────────────────────────

#[test]
fn hit_test_does_not_mutate_the_tree() {
    let root = group(
        rect(0.0, 0.0, 100.0, 100.0),
        Some(Msg::Container),
        vec![leaf(Some(Msg::First), rect(0.0, 0.0, 50.0, 50.0))],
    );
    let before = format!("{root:?}");
    let _ = perform_hit_test(&root, 25.0, 25.0);
    let _ = perform_hit_test(&root, 75.0, 75.0);
    // `&View` 已在类型层面禁止修改；这里再断言运行时内容确实未变。
    assert_eq!(format!("{root:?}"), before);
}

// ── 与 layout 串联（端到端） ────────────────────────────────────────────────

#[test]
fn hit_after_real_layout_matches_dp_geometry() {
    // 1080x1920 窗口、density=2：Dp(200x100) + margin 20dp
    // → 物理 400x200、原点 (40, 40)。
    let button = View::text_view("button")
        .set_on_click_listener(Msg::First)
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(200.0),
            height: LayoutDimension::Dp(100.0),
            margin: velm::view::EdgeInsets::all(20.0),
        });
    let mut root = View::linear_layout(Orientation::Vertical, vec![button]);
    measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);

    // 命中按钮中心。
    assert_eq!(perform_hit_test(&root, 100.0, 100.0), Some(Msg::First));
    // 边界内：左下角。
    assert_eq!(perform_hit_test(&root, 40.0, 240.0), Some(Msg::First));
    // 按钮下方空白：根容器无监听 → None。
    assert_eq!(perform_hit_test(&root, 100.0, 500.0), None);
    // 按钮左侧 margin 区域：None。
    assert_eq!(perform_hit_test(&root, 10.0, 100.0), None);
}
