//! view/widget.rs — Android 风格复合组件（host 可测，无任何 android 符号）。
//!
//! 组件差异收敛到 [`WidgetKind`] 枚举，外层只有唯一的 [`View::Widget`] 变体
//! （见 `view/mod.rs`），使新增组件只需加一个 `kind`，而非在 `measure` / `place` /
//! `scene` / `hit_test` 各加一个完整分支——避免 `View` 枚举爆炸。
//!
//! 所有组件共享 [`CommonStyle`]（布局参数 / 背景 / 圆角 / 描边 / 内边距 / 点击消息 /
//! 已布局矩形），密度相关量（圆角 dp、描边 dp、内边距 dp）均在绘制阶段乘 density
//! 换算为像素（ADR-12，参考 Android `DisplayMetrics`）。

use peniko::Color;

use super::View;
use super::params::{
    Background, EdgeInsets, LayoutDimension, LayoutParams, Orientation, Rect, Stroke,
};

/// 所有组件共用的样式与布局状态（绘制 / 布局 / 命中测试共享同一份数据）。
#[derive(Clone, Debug)]
pub struct CommonStyle<Msg> {
    /// 宽高规格与外边距。
    pub layout_params: LayoutParams,
    /// 填充色（圆角见 [`corner_radius_dp`]）。
    pub background: Background,
    /// 圆角半径（dp；绘制阶段乘 density，密度感知）。
    pub corner_radius_dp: f32,
    /// 描边（边框，dp；绘制阶段乘 density）。
    pub stroke: Stroke,
    /// 四向内边距（dp）。
    pub padding: EdgeInsets,
    /// 点击命中所产生的消息；`None` 表示该节点不响应点击。
    pub on_click_listener: Option<Msg>,
    /// 布局阶段写入的绝对像素矩形；未布局前为全零。
    pub computed_rect: Rect,
}

impl<Msg> CommonStyle<Msg> {
    /// 以默认样式构造共用状态。
    pub(crate) fn new() -> Self {
        Self {
            layout_params: LayoutParams::default(),
            background: Background::default(),
            corner_radius_dp: 0.0,
            stroke: Stroke::default(),
            padding: EdgeInsets::default(),
            on_click_listener: None,
            computed_rect: Rect::default(),
        }
    }
}

/// 一个复合组件节点：共用样式 + 具体类型。
#[derive(Clone, Debug)]
pub struct WidgetView<Msg> {
    /// 共用样式与布局状态。
    pub common: CommonStyle<Msg>,
    /// 组件具体类型与数据。
    pub kind: WidgetKind<Msg>,
}

/// 组件类型枚举（新增组件只需加一个变体 + 一个规格结构）。
#[derive(Clone, Debug)]
pub enum WidgetKind<Msg> {
    /// 按钮：圆角底色 + 居中文字（参考 Material Button）。
    Button(ButtonSpec),
    /// 卡片：圆角 + 阴影（elevation）+ 内边距的容器（参考 CardView）。
    Card(CardSpec<Msg>),
    /// 图片：占位底色 + 缩放类型（真实图片解码留待后续，v1 仅占位，参考 ImageView）。
    Image(ImageSpec),
    /// 进度条：轨道 + 进度填充（参考 ProgressBar）。
    Progress(ProgressSpec),
    /// 复选框：方框 + 勾选标记（参考 CheckBox）。
    Check(CheckSpec),
    /// 开关：轨道 + 滑块（参考 Switch）。
    Switch(SwitchSpec),
    /// 占位间隔：固定 dp 尺寸（参考 Space）。
    Space(SpaceSpec),
    /// 输入框：底色 + 边框 + 提示文字（IME 接入前仅视觉，参考 EditText）。
    Edit(EditSpec),
}

/// 按钮规格。
#[derive(Clone, Debug)]
pub struct ButtonSpec {
    /// 按钮文字。
    pub text: String,
    /// 字号（sp）。
    pub text_size: f32,
    /// 文字颜色。
    pub text_color: Color,
}

/// 卡片规格（容器）。
#[derive(Clone, Debug)]
pub struct CardSpec<Msg> {
    /// 子节点排列方向。
    pub orientation: Orientation,
    /// 子节点列表。
    pub children: Vec<View<Msg>>,
    /// 阴影高度（dp；绘制阶段乘 density）。
    pub elevation_dp: f32,
}

/// 图片缩放类型（参考 Android `ImageView.ScaleType`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageScale {
    /// 等比缩放居中（fit + center）。
    FitCenter,
    /// 居中不缩放（center）。
    Center,
    /// 等比填满裁剪（centerCrop）。
    CenterCrop,
}

/// 图片规格。
#[derive(Clone, Debug)]
pub struct ImageSpec {
    /// 占位底色（真实图片解码留待后续）。
    pub placeholder: Color,
    /// WrapContent 时的默认边长（dp，正方形）。
    pub default_size_dp: f32,
    /// 缩放类型。
    pub scale: ImageScale,
}

/// 进度条方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressOrientation {
    /// 水平条。
    Horizontal,
    /// 环形。
    Circular,
}

/// 进度条规格。
#[derive(Clone, Debug)]
pub struct ProgressSpec {
    /// 进度 [0.0, 1.0]。
    pub progress: f32,
    /// 方向。
    pub orientation: ProgressOrientation,
    /// 水平条厚度 / 环形直径基准（dp）。
    pub thickness_dp: f32,
    /// 环形直径（dp，仅 Circular 使用，默认 48）。
    pub size_dp: f32,
    /// 轨道色。
    pub track_color: Color,
    /// 进度色。
    pub progress_color: Color,
}

/// 复选框规格。
#[derive(Clone, Debug)]
pub struct CheckSpec {
    /// 是否勾选。
    pub checked: bool,
    /// 方框边长（dp，默认 24）。
    pub size_dp: f32,
    /// 方框描边色。
    pub box_color: Color,
    /// 勾选标记色。
    pub check_color: Color,
}

/// 开关规格。
#[derive(Clone, Debug)]
pub struct SwitchSpec {
    /// 是否开启。
    pub checked: bool,
    /// 轨道宽度（dp，默认 52）。
    pub width_dp: f32,
    /// 轨道高度（dp，默认 32）。
    pub height_dp: f32,
    /// 开启轨道色。
    pub on_color: Color,
    /// 关闭轨道色。
    pub off_color: Color,
    /// 滑块色。
    pub thumb_color: Color,
}

/// 占位间隔规格。
#[derive(Clone, Debug)]
pub struct SpaceSpec {
    /// 宽度（dp，默认 8）。
    pub width_dp: f32,
    /// 高度（dp，默认 8）。
    pub height_dp: f32,
}

/// 输入框规格。
#[derive(Clone, Debug)]
pub struct EditSpec {
    /// 当前文本（IME 接入前为空）。
    pub text: String,
    /// 提示文字（文本为空时显示）。
    pub hint: String,
    /// 字号（sp）。
    pub text_size: f32,
    /// 文本颜色。
    pub text_color: Color,
    /// 提示颜色。
    pub hint_color: Color,
}

// ── 构造器与默认值 ─────────────────────────────────────────────────────────────

impl<Msg> WidgetView<Msg> {
    /// 内部构造：给定 kind 与共用样式默认值。
    pub(crate) fn new(kind: WidgetKind<Msg>) -> Self {
        Self {
            common: CommonStyle::new(),
            kind,
        }
    }

    /// 仅对承载文本的组件（Button / Edit）设置字号（sp）；其它 kind 为 no-op。
    pub(crate) fn set_text_size(&mut self, size: f32) {
        match &mut self.kind {
            WidgetKind::Button(b) => b.text_size = size,
            WidgetKind::Edit(e) => e.text_size = size,
            _ => {}
        }
    }

    /// 仅对承载文本的组件（Button / Edit）设置文字颜色；其它 kind 为 no-op。
    pub(crate) fn set_text_color(&mut self, color: Color) {
        match &mut self.kind {
            WidgetKind::Button(b) => b.text_color = color,
            WidgetKind::Edit(e) => e.text_color = color,
            _ => {}
        }
    }
}

/// 默认占位灰（#9E9E9E）。
pub(crate) const DEFAULT_PLACEHOLDER: Color = Color::from_rgba8(0x9E, 0x9E, 0x9E, 0xFF);
/// 默认主题色（#2196F3，Material Blue 500）。
pub(crate) const DEFAULT_ACCENT: Color = Color::from_rgba8(0x21, 0x96, 0xF3, 0xFF);
/// 默认轨道灰（#BDBDBD）。
pub(crate) const DEFAULT_TRACK: Color = Color::from_rgba8(0xBD, 0xBD, 0xBD, 0xFF);
/// 默认文字黑（#212121）。
pub(crate) const DEFAULT_TEXT_DARK: Color = Color::from_rgba8(0x21, 0x21, 0x21, 0xFF);
/// 默认提示灰（#757575）。
pub(crate) const DEFAULT_HINT: Color = Color::from_rgba8(0x75, 0x75, 0x75, 0xFF);

/// 按钮默认内边距（dp）：水平 16、垂直 8，配合 14sp 文字得到约 36dp 高（Material）。
pub(crate) fn button_padding() -> EdgeInsets {
    EdgeInsets {
        left: 16.0,
        right: 16.0,
        top: 8.0,
        bottom: 8.0,
    }
}

/// 卡片默认圆角（dp）。
pub(crate) const DEFAULT_CARD_RADIUS_DP: f32 = 8.0;
/// 卡片默认阴影高度（dp）。
pub(crate) const DEFAULT_CARD_ELEVATION_DP: f32 = 4.0;
/// 输入框默认边框 dp。
pub(crate) const DEFAULT_EDIT_STROKE_DP: f32 = 1.0;
/// 复选框默认边长（dp）。
pub(crate) const DEFAULT_CHECK_SIZE_DP: f32 = 24.0;
/// 开关默认尺寸（dp）。
pub(crate) const DEFAULT_SWITCH_W_DP: f32 = 52.0;
pub(crate) const DEFAULT_SWITCH_H_DP: f32 = 32.0;
/// 进度条默认厚度（dp）。
pub(crate) const DEFAULT_PROGRESS_THICKNESS_DP: f32 = 4.0;
/// 进度条环形默认直径（dp）。
pub(crate) const DEFAULT_PROGRESS_SIZE_DP: f32 = 48.0;
/// 占位间隔默认尺寸（dp）。
pub(crate) const DEFAULT_SPACE_DP: f32 = 8.0;
/// 复合组件默认字号（sp）。
pub(crate) const DEFAULT_WIDGET_TEXT_SP: f32 = 14.0;
/// 图片默认边长（dp）。
pub(crate) const DEFAULT_IMAGE_SIZE_DP: f32 = 48.0;

impl<Msg> View<Msg> {
    /// 按钮：圆角底色 + 居中文字；默认 14sp 文字、16/8dp 内边距、无背景色（由
    /// `set_background` 设置）。点击消息经 [`View::set_on_click_listener`] 绑定。
    pub fn button(text: impl Into<String>) -> Self {
        View::Widget(WidgetView::new(WidgetKind::Button(ButtonSpec {
            text: text.into(),
            text_size: DEFAULT_WIDGET_TEXT_SP,
            text_color: DEFAULT_TEXT_DARK,
        })))
        .with_widget(|w| w.common.padding = button_padding())
    }

    /// 卡片（纵向容器）：圆角 + 阴影 + 内边距；子节点按纵向排列。
    pub fn card(children: Vec<View<Msg>>) -> Self {
        View::card_with(Orientation::Vertical, children)
    }

    /// 卡片（指定方向容器）。
    pub fn card_with(orientation: Orientation, children: Vec<View<Msg>>) -> Self {
        View::Widget(WidgetView::new(WidgetKind::Card(CardSpec {
            orientation,
            children,
            elevation_dp: DEFAULT_CARD_ELEVATION_DP,
        })))
        .with_widget(|w| {
            w.common.corner_radius_dp = DEFAULT_CARD_RADIUS_DP;
            w.common.background.color = Some(Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF));
            w.common.padding = EdgeInsets::all(8.0);
        })
    }

    /// 图片占位：默认 48dp 边长、居中缩放。占位色经 [`View::set_image_placeholder`] 设置。
    pub fn image_view() -> Self {
        View::Widget(WidgetView::new(WidgetKind::Image(ImageSpec {
            placeholder: DEFAULT_PLACEHOLDER,
            default_size_dp: DEFAULT_IMAGE_SIZE_DP,
            scale: ImageScale::FitCenter,
        })))
    }

    /// 水平进度条：进度 0、4dp 厚、主题色填充。
    pub fn progress_bar() -> Self {
        View::Widget(WidgetView::new(WidgetKind::Progress(ProgressSpec {
            progress: 0.0,
            orientation: ProgressOrientation::Horizontal,
            thickness_dp: DEFAULT_PROGRESS_THICKNESS_DP,
            size_dp: DEFAULT_PROGRESS_SIZE_DP,
            track_color: DEFAULT_TRACK,
            progress_color: DEFAULT_ACCENT,
        })))
    }

    /// 复选框：默认 24dp、未勾选。
    pub fn check_box(checked: bool) -> Self {
        View::Widget(WidgetView::new(WidgetKind::Check(CheckSpec {
            checked,
            size_dp: DEFAULT_CHECK_SIZE_DP,
            box_color: DEFAULT_ACCENT,
            check_color: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
        })))
    }

    /// 开关：默认 52×32dp、关闭态。
    pub fn switch(checked: bool) -> Self {
        View::Widget(WidgetView::new(WidgetKind::Switch(SwitchSpec {
            checked,
            width_dp: DEFAULT_SWITCH_W_DP,
            height_dp: DEFAULT_SWITCH_H_DP,
            on_color: DEFAULT_ACCENT,
            off_color: DEFAULT_TRACK,
            thumb_color: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
        })))
    }

    /// 占位间隔：默认 8×8dp。
    pub fn space() -> Self {
        View::Widget(WidgetView::new(WidgetKind::Space(SpaceSpec {
            width_dp: DEFAULT_SPACE_DP,
            height_dp: DEFAULT_SPACE_DP,
        })))
    }

    /// 输入框：默认 14sp 文字、灰色提示、1dp 边框。
    pub fn edit_text(hint: impl Into<String>) -> Self {
        View::Widget(WidgetView::new(WidgetKind::Edit(EditSpec {
            text: String::new(),
            hint: hint.into(),
            text_size: DEFAULT_WIDGET_TEXT_SP,
            text_color: DEFAULT_TEXT_DARK,
            hint_color: DEFAULT_HINT,
        })))
        .with_widget(|w| {
            w.common.padding = EdgeInsets::all(12.0);
            w.common.stroke.width_dp = DEFAULT_EDIT_STROKE_DP;
            w.common.stroke.color = Some(DEFAULT_TRACK);
        })
    }

    /// 仅在节点是 `Widget` 时应用闭包；其它节点原样返回（链式构造安全）。
    pub(crate) fn with_widget(mut self, f: impl FnOnce(&mut WidgetView<Msg>)) -> Self {
        if let View::Widget(w) = &mut self {
            f(w);
        }
        self
    }

    /// 设置组件圆角（dp，密度感知）。仅对 `Widget` 生效，其它节点为 no-op。
    pub fn set_corner_radius_dp(self, radius_dp: f32) -> Self {
        self.with_widget(|w| w.common.corner_radius_dp = radius_dp)
    }

    /// 设置组件描边（dp）。对所有节点生效（TextView / ViewGroup 也已支持 Stroke）。
    pub fn set_stroke(self, color: Color, width_dp: f32) -> Self {
        let stroke = Stroke {
            color: Some(color),
            width_dp,
        };
        match self {
            View::TextView(mut tv) => {
                tv.stroke = stroke;
                View::TextView(tv)
            }
            View::ViewGroup(mut vg) => {
                vg.stroke = stroke;
                View::ViewGroup(vg)
            }
            View::Widget(mut w) => {
                w.common.stroke = stroke;
                View::Widget(w)
            }
        }
    }

    /// 设置组件内边距（dp）。对所有节点生效。
    pub fn set_padding(self, padding: EdgeInsets) -> Self {
        match self {
            View::TextView(mut tv) => {
                tv.padding = padding;
                View::TextView(tv)
            }
            View::ViewGroup(mut vg) => {
                vg.padding = padding;
                View::ViewGroup(vg)
            }
            View::Widget(mut w) => {
                w.common.padding = padding;
                View::Widget(w)
            }
        }
    }

    /// 设置进度条进度 [0.0, 1.0]。仅对 Progress 生效。
    pub fn set_progress(self, progress: f32) -> Self {
        self.with_widget(|w| {
            if let WidgetKind::Progress(p) = &mut w.kind {
                p.progress = progress.clamp(0.0, 1.0);
            }
        })
    }

    /// 设置复选框勾选态。仅对 Check 生效。
    pub fn set_checked(self, checked: bool) -> Self {
        self.with_widget(|w| {
            if let WidgetKind::Check(c) = &mut w.kind {
                c.checked = checked;
            }
        })
    }

    /// 设置图片占位色。仅对 Image 生效。
    pub fn set_image_placeholder(self, color: Color) -> Self {
        self.with_widget(|w| {
            if let WidgetKind::Image(img) = &mut w.kind {
                img.placeholder = color;
            }
        })
    }

    /// 设置占位间隔尺寸（dp）。仅对 Space 生效。
    pub fn set_space_size(self, width_dp: f32, height_dp: f32) -> Self {
        self.with_widget(|w| {
            if let WidgetKind::Space(s) = &mut w.kind {
                s.width_dp = width_dp;
                s.height_dp = height_dp;
            }
        })
    }

    /// 便捷构造 Dp 尺寸布局参数（配合 `set_layout_params`）。
    pub fn dp_params(width: f32, height: f32) -> LayoutParams {
        LayoutParams {
            width: LayoutDimension::Dp(width),
            height: LayoutDimension::Dp(height),
            margin: EdgeInsets::default(),
        }
    }
}
