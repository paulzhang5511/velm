//! view/mod.rs — View 视图树与链式构造器（SPEC §7.2）。
//!
//! 本模块为**纯 host 逻辑**：只依赖 `peniko::Color`，不出现任何 android FFI
//! 符号，可在开发机直接 `cargo test`（§10.2 硬约束）。每帧由 `Activity::on_draw`
//! 重建整棵树，节点随作用域释放，不实现 `Drop` 特殊语义（SPEC §7.2 约束）。
//!
//! 组件差异收敛到 [`WidgetKind`]：外层仅一个 [`View::Widget`] 变体，新增组件只加
//! 一个 `kind`，避免 `View` 枚举爆炸（见 `view/widget.rs`）。

mod params;
mod text_view;
mod view_group;
mod widget;

pub use params::{
    Background, EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect, Stroke,
};
pub use text_view::{DEFAULT_TEXT_COLOR, DEFAULT_TEXT_SIZE, TextView};
pub use view_group::ViewGroup;
pub use widget::{
    ButtonSpec, CardSpec, CheckSpec, CommonStyle, EditSpec, ImageScale, ImageSpec,
    ProgressOrientation, ProgressSpec, SpaceSpec, SwitchSpec, WidgetKind, WidgetView,
};

use peniko::Color;

/// 视图树节点：叶子文本、容器或复合组件。
#[derive(Clone, Debug)]
pub enum View<Msg> {
    /// 叶子文本节点。
    TextView(TextView<Msg>),
    /// 容器节点（LinearLayout）。
    ViewGroup(ViewGroup<Msg>),
    /// 复合组件（Button / Card / ImageView / ProgressBar / CheckBox / Switch / Space / EditText）。
    Widget(WidgetView<Msg>),
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

    /// 设置字号（sp）。对 TextView、Button、EditText 生效，其它节点为 no-op 并输出 trace 日志。
    pub fn set_text_size(self, size: f32) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.text_size = size;
                View::TextView(tv)
            }
            View::Widget(mut w) => {
                w.set_text_size(size);
                View::Widget(w)
            }
            other => {
                log::trace!("[View] set_text_size ignored: node is not a text-bearing view");
                other
            }
        }
    }

    /// 设置文字颜色。对 TextView、Button、EditText 生效，其它节点为 no-op 并输出 trace 日志。
    pub fn set_text_color(self, color: Color) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.text_color = color;
                View::TextView(tv)
            }
            View::Widget(mut w) => {
                w.set_text_color(color);
                View::Widget(w)
            }
            other => {
                log::trace!("[View] set_text_color ignored: node is not a text-bearing view");
                other
            }
        }
    }

    /// 设置背景填充色与圆角半径（px）；TextView 与 ViewGroup 均生效（ADR-10）。
    /// 复合组件请改用 [`View::set_background_color`]（圆角以 dp 计，密度感知）。
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
            View::Widget(mut w) => {
                w.common.background = background;
                View::Widget(w)
            }
        }
    }

    /// 设置复合组件背景填充色（圆角请用 [`View::set_corner_radius_dp`]，dp 计、密度感知）。
    pub fn set_background_color(self, color: Color) -> Self {
        self.with_widget(|w| w.common.background.color = Some(color))
    }

    /// 绑定点击消息：命中该节点时产生 `message`；三种节点均可绑定。
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
            View::Widget(mut w) => {
                w.common.on_click_listener = Some(message);
                View::Widget(w)
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
            View::Widget(mut w) => {
                w.common.layout_params = params;
                View::Widget(w)
            }
        }
    }
}
