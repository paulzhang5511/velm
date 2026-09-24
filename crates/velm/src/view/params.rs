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

/// 描边样式（Android 的 stroke / 边框）。
///
/// 用于 CardView 的轮廓、EditText 的输入边框、CheckBox 的方框等。线宽以 dp 声明，
/// 布局阶段乘 density 换算为像素（与 margin / dp 同一套规则，ADR-12）。`color` 为
/// `None` 时不描边。
#[derive(Clone, Copy, Debug, Default)]
pub struct Stroke {
    /// 描边色；`None` 表示无描边。
    pub color: Option<Color>,
    /// 线宽（dp；布局阶段乘 density）。
    pub width_dp: f32,
}

/// 禁用态整体透明度（对齐 Android `View` 的 `DISABLED_ALPHA` = 0.5）。
///
/// 仅作用于颜色（不改几何）：禁用节点的填充 / 描边 / 文字统一按此系数乘 alpha。
pub const DISABLED_ALPHA: f32 = 0.5;

/// 按下态背景压暗系数（0.85 ≈ Android `state_pressed` 的 ripple / scrim 观感）。
pub const PRESSED_SCALE: f32 = 0.85;

/// 交互态：是否可响应点击、是否处于按下态（参考 Android
/// `View.setEnabled` / `state_enabled` / `state_pressed`）。
///
/// v1.1 增量（ADR-14）：
///
/// - `enabled = false`：节点**不响应点击**（hit-test 视为透明，事件穿透到下层兄弟），
///   且绘制时颜色按 [`DISABLED_ALPHA`] 降透明；
/// - `pressed = true`：仅**背景填充**按 [`PRESSED_SCALE`] 压暗（文字 / 描边不变），
///   由引擎在 `ACTION_DOWN` → `ACTION_UP/CANCEL` 期间维护；
/// - 两者同时置位时以 `enabled` 优先（禁用节点忽略按下态）。
///
/// 语义与 Android 一致：**只影响本节点**，不自动向下传播到子节点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interaction {
    /// 是否可交互；`false` 时不响应点击且视觉降透明。
    pub enabled: bool,
    /// 是否处于按下态。
    pub pressed: bool,
}

impl Default for Interaction {
    /// 默认可用、未按下（普通态）。
    fn default() -> Self {
        Self {
            enabled: true,
            pressed: false,
        }
    }
}

impl Interaction {
    /// 是否处于普通态（可用且未按下）——此态下绘制颜色不做任何变换。
    pub fn is_normal(&self) -> bool {
        self.enabled && !self.pressed
    }

    /// 背景填充色变换：禁用 → 降透明；按下 → 压暗；否则原色。
    pub fn tint_fill(&self, color: Color) -> Color {
        if !self.enabled {
            return scale_alpha(color, DISABLED_ALPHA);
        }
        if self.pressed {
            return scale_rgb(color, PRESSED_SCALE);
        }
        color
    }

    /// 内容色（文字 / 描边 / 阴影）变换：仅禁用时降透明。
    pub fn tint_content(&self, color: Color) -> Color {
        if self.enabled {
            color
        } else {
            scale_alpha(color, DISABLED_ALPHA)
        }
    }
}

/// 按系数缩放颜色 alpha（其余通道不变）。
fn scale_alpha(color: Color, factor: f32) -> Color {
    let [r, g, b, a] = color.to_rgba8().to_u8_array();
    Color::from_rgba8(r, g, b, (a as f32 * factor).round() as u8)
}

/// 按系数缩放颜色 RGB（alpha 不变）——用于按下态压暗。
fn scale_rgb(color: Color, factor: f32) -> Color {
    let [r, g, b, a] = color.to_rgba8().to_u8_array();
    Color::from_rgba8(
        (r as f32 * factor).round() as u8,
        (g as f32 * factor).round() as u8,
        (b as f32 * factor).round() as u8,
        a,
    )
}
