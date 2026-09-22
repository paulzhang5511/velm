//! view/mod.rs — View 视图树与链式构造器（SPEC §7.2）。
//!
//! 本模块为**纯 host 逻辑**：只依赖 `peniko::Color`，不出现任何 android FFI
//! 符号，可在开发机直接 `cargo test`（§10.2 硬约束）。每帧由 `Activity::on_draw`
//! 重建整棵树，节点随作用域释放，不实现 `Drop` 特殊语义（SPEC §7.2 约束）。

mod params;
mod text_view;
mod view_group;

pub use params::{Background, EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect};
pub use text_view::{DEFAULT_TEXT_COLOR, DEFAULT_TEXT_SIZE, TextView};
pub use view_group::ViewGroup;

use peniko::Color;

/// 视图树节点：叶子文本或容器。
#[derive(Clone, Debug)]
pub enum View<Msg> {
    /// 叶子文本节点。
    TextView(TextView<Msg>),
    /// 容器节点（LinearLayout）。
    ViewGroup(ViewGroup<Msg>),
}

impl<Msg> View<Msg> {
    /// LinearLayout：按 `orientation` 排列 `children`，宽高默认 `MatchParent`
    /// （根容器语义；需要 WrapContent / Dp 时用 [`View::set_layout_params`] 覆盖）。
    pub fn linear_layout(orientation: Orientation, children: Vec<View<Msg>>) -> Self {
        let layout_params = LayoutParams {
            width: LayoutDimension::MatchParent,
            height: LayoutDimension::MatchParent,
            margin: EdgeInsets::default(),
        };
        View::ViewGroup(ViewGroup::new(orientation, layout_params, children))
    }

    /// 文本节点：默认 16sp 白字、无背景、布局参数取 [`LayoutParams::default()`]。
    pub fn text_view(text: impl Into<String>) -> Self {
        View::TextView(TextView::new(text.into()))
    }

    /// 设置字号（sp）。仅对 TextView 生效，其它节点为 no-op 并输出 trace 日志。
    pub fn set_text_size(self, size: f32) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.text_size = size;
                View::TextView(tv)
            }
            other => {
                log::trace!("[View] set_text_size ignored: node is not a TextView");
                other
            }
        }
    }

    /// 设置文字颜色。仅对 TextView 生效，其它节点为 no-op 并输出 trace 日志。
    pub fn set_text_color(self, color: Color) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.text_color = color;
                View::TextView(tv)
            }
            other => {
                log::trace!("[View] set_text_color ignored: node is not a TextView");
                other
            }
        }
    }

    /// 设置背景填充色与圆角半径（px）；TextView 与 ViewGroup 均生效（ADR-10）。
    pub fn set_background(self, color: Color, corner_radius: f32) -> Self {
        let background = Background {
            color: Some(color),
            corner_radius,
        };
        match self {
            View::TextView(mut tv) => {
                tv.background = background;
                View::TextView(tv)
            }
            View::ViewGroup(mut vg) => {
                vg.background = background;
                View::ViewGroup(vg)
            }
        }
    }

    /// 绑定点击消息：命中该节点时产生 `message`；两种节点均可绑定。
    pub fn set_on_click_listener(self, message: Msg) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.on_click_listener = Some(message);
                View::TextView(tv)
            }
            View::ViewGroup(mut vg) => {
                vg.on_click_listener = Some(message);
                View::ViewGroup(vg)
            }
        }
    }

    /// 覆盖布局参数（宽高规格与四向外边距）。
    pub fn set_layout_params(self, params: LayoutParams) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.layout_params = params;
                View::TextView(tv)
            }
            View::ViewGroup(mut vg) => {
                vg.layout_params = params;
                View::ViewGroup(vg)
            }
        }
    }
}
