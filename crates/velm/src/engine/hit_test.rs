//! engine/hit_test.rs — 点击命中测试（host 可测，无任何 android 符号）。

use crate::view::{View, WidgetKind};

/// 在已完成布局的 View 树中执行 DFS 命中测试，返回被点击节点绑定的消息。
///
/// 规则（SPEC §7.5）：
///
/// - 容器自身 rect 不包含坐标时整棵子树直接排除（子节点坐标必然落在父内，
///   提前剪枝避免无谓遍历）；
/// - 子节点按**逆序**探测，使后添加（绘制在上层）的子节点优先响应；
/// - 子节点未命中时回落到容器自身的点击监听；
/// - 命中判定使用 `Rect::contains` 的**闭区间**语义，边界点算命中。
///
/// 坐标系：`x/y` 必须是布局输出的窗口物理像素绝对坐标，与 `MotionEvent`
/// 的 x/y 同坐标系（ADR-12）。若未来引入 viewport 变换或刘海偏移，必须在
/// 调用前先变换到布局坐标系。
///
/// # 透明容器（T7 定稿的语义澄清）
///
/// 「回落到容器自身监听」的回落在 `find_map` 内部完成：某个子节点若自身
/// 及后代都未产生消息（例如它只是纯布局容器、没绑监听），`find_map` 会继续
/// 探测**更下层的兄弟节点**，而不是就此停住。即：无监听的容器对点击是
/// 「透明」的，事件会穿透到它下面的兄弟节点；只有容器**绑定了监听**时，
/// 才会真正拦下这次点击。
pub fn perform_hit_test<Msg: Clone>(root: &View<Msg>, x: f32, y: f32) -> Option<Msg> {
    if !node_rect(root).contains(x, y) {
        return None;
    }
    // 容器：逆序探测子节点，未命中则回落到自身监听。
    if let Some(children) = node_children(root) {
        for child in children.iter().rev() {
            if let Some(msg) = perform_hit_test(child, x, y) {
                return Some(msg);
            }
        }
    }
    node_listener(root)
}

/// 节点布局后的绝对像素矩形（所有节点类型统一）。
fn node_rect<Msg>(node: &View<Msg>) -> crate::view::Rect {
    match node {
        View::TextView(tv) => tv.computed_rect,
        View::ViewGroup(vg) => vg.computed_rect,
        View::Widget(w) => w.common.computed_rect,
    }
}

/// 节点的点击消息（未绑定为 `None`）。
fn node_listener<Msg: Clone>(node: &View<Msg>) -> Option<Msg> {
    match node {
        View::TextView(tv) => tv.on_click_listener.clone(),
        View::ViewGroup(vg) => vg.on_click_listener.clone(),
        View::Widget(w) => w.common.on_click_listener.clone(),
    }
}

/// 容器的子节点切片；非容器返回 `None`（命中测试不再下钻）。
fn node_children<Msg>(node: &View<Msg>) -> Option<&[View<Msg>]> {
    match node {
        View::ViewGroup(vg) => Some(&vg.children),
        View::Widget(w) => match &w.kind {
            WidgetKind::Card(card) => Some(&card.children),
            _ => None,
        },
        View::TextView(_) => None,
    }
}
