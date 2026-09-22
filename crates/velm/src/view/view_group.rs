//! view/view_group.rs — 容器视图节点（LinearLayout）。

use super::View;
use super::params::{Background, LayoutParams, Orientation, Rect};

/// 容器节点：按 [`Orientation`] 顺序排列子节点（v1 手写 LinearLayout，ADR-05）。
///
/// `Msg` 在此不加任何 bound（SPEC §7.2）。
#[derive(Clone, Debug)]
pub struct ViewGroup<Msg> {
    /// 子节点排列方向。
    pub orientation: Orientation,
    /// 背景填充与圆角；默认无背景。
    pub background: Background,
    /// 布局参数（宽高规格与外边距）。
    pub layout_params: LayoutParams,
    /// 子节点列表；绘制与命中测试均按「后添加者在上层」处理。
    pub children: Vec<View<Msg>>,
    /// 子节点未命中时回落的点击消息。
    pub on_click_listener: Option<Msg>,
    /// 布局阶段写入的绝对像素矩形；未布局前为全零。
    pub computed_rect: Rect,
}

impl<Msg> ViewGroup<Msg> {
    /// 以给定方向 / 布局参数 / 子节点构造容器。
    pub(crate) fn new(
        orientation: Orientation,
        layout_params: LayoutParams,
        children: Vec<View<Msg>>,
    ) -> Self {
        Self {
            orientation,
            background: Background::default(),
            layout_params,
            children,
            on_click_listener: None,
            computed_rect: Rect::default(),
        }
    }
}
