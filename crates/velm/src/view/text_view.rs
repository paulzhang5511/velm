//! view/text_view.rs — 文本视图节点（叶子节点）。

use peniko::Color;

use super::params::{Background, EdgeInsets, LayoutParams, Rect, Stroke};

/// v1 默认字号（sp；布局阶段乘 density，ADR-12）。
pub const DEFAULT_TEXT_SIZE: f32 = 16.0;

/// v1 默认文字颜色：不透明白（ADR-10）。
pub const DEFAULT_TEXT_COLOR: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);

/// 叶子文本节点：一行文本 + 可选背景 + 可选点击消息。
///
/// `Msg` 在此不加任何 bound（SPEC §7.2）：仅 hit-test 返回消息时要求 `Clone`。
#[derive(Clone, Debug)]
pub struct TextView<Msg> {
    /// 文本内容（v1 单行，不自动换行）。
    pub text: String,
    /// 字号，单位 sp（布局阶段乘 density）。
    pub text_size: f32,
    /// 文字颜色。
    pub text_color: Color,
    /// 背景填充与圆角；默认无背景。
    pub background: Background,
    /// 描边（边框）；默认无描边。
    pub stroke: Stroke,
    /// 四向内边距（dp；布局阶段乘 density，ADR-12）。Android 组件靠它内缩文本 / 内容。
    pub padding: EdgeInsets,
    /// 布局参数（宽高规格与外边距）。
    pub layout_params: LayoutParams,
    /// 点击命中所产生的消息；`None` 表示该节点不响应点击。
    pub on_click_listener: Option<Msg>,
    /// 布局阶段写入的绝对像素矩形；未布局前为全零。
    pub computed_rect: Rect,
}

impl<Msg> TextView<Msg> {
    /// 以默认样式构造文本节点（16sp 白字、无背景、WrapContent）。
    pub(crate) fn new(text: String) -> Self {
        Self {
            text,
            text_size: DEFAULT_TEXT_SIZE,
            text_color: DEFAULT_TEXT_COLOR,
            background: Background::default(),
            stroke: Stroke::default(),
            padding: EdgeInsets::default(),
            layout_params: LayoutParams::default(),
            on_click_listener: None,
            computed_rect: Rect::default(),
        }
    }
}
