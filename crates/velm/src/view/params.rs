//! view/params.rs — 布局参数与矩形（host 可测，无任何 android 符号）。
//!
//! 长度单位约定（ADR-12）：构造阶段一律为 dp/sp，`layout::measure` 阶段统一
//! 乘 density 换算为物理像素；`Rect` 已经是换算后的物理像素绝对坐标，与
//! NDK `MotionEvent` 的 x/y 同坐标系（hit-test 正确性的前提）。

use peniko::Color;

/// 视图宽 / 高尺寸规格。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutDimension {
    /// 填满父节点的可用内容区。
    MatchParent,
    /// 由自身内容决定：TextView 取文本估算尺寸，ViewGroup 取子节点求和。
    WrapContent,
    /// 固定 dp 值（布局阶段乘 density）。
    Dp(f32),
}

/// LinearLayout 子节点排列方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// 沿 x 轴从左到右排列。
    Horizontal,
    /// 沿 y 轴从上到下排列。
    Vertical,
}

/// 四向外边距（dp；布局阶段乘 density）。
///
/// v1 无 padding，节点可用内容区 = 父内容区扣除自身四向 margin。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    /// 四向等距外边距。
    pub fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }
}

/// 视图布局参数：宽高规格 + 四向外边距。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutParams {
    /// 宽度规格。
    pub width: LayoutDimension,
    /// 高度规格。
    pub height: LayoutDimension,
    /// 四向外边距。
    pub margin: EdgeInsets,
}

impl Default for LayoutParams {
    /// 默认宽高均为 [`LayoutDimension::WrapContent`]、四向 margin 为 0（SPEC §7.2）。
    fn default() -> Self {
        Self {
            width: LayoutDimension::WrapContent,
            height: LayoutDimension::WrapContent,
            margin: EdgeInsets::default(),
        }
    }
}

/// 布局完成后的矩形；坐标为窗口物理像素绝对坐标（ADR-12）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// 闭区间命中判定：落在边界算命中（SPEC §7.5）。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// v1 最小背景样式（ADR-10）：纯色填充 + 圆角半径（px）。
///
/// `color` 为 `None` 时不绘制背景（透明，仅容器本身可见子节点）。
#[derive(Clone, Copy, Debug, Default)]
pub struct Background {
    /// 填充色；`None` 表示无背景。
    pub color: Option<Color>,
    /// 圆角半径（物理像素）。
    pub corner_radius: f32,
}
