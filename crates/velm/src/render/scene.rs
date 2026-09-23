//! render/scene.rs — 视图树 → 绘制指令（**纯逻辑，host 可编译可测**）。
//!
//! 渲染被切成两段，是为了把「画什么、画在哪」与「怎么用 vello 画」解耦：
//!
//! - 本模块：只读视图树，产出与 GPU 无关的 [`DrawCommand`] 序列（顺序即
//!   画家算法，先产出者在下层）；可在 host 单测（SPEC §10.2）。
//! - `render::vello_renderer`（android-only）：把指令编码进 vello `Scene`。
//!
//! 指令里一切长度均为**物理像素**：`computed_rect` 已是像素，sp 经
//! [`sp_to_px`] 换算，与布局、hit-test 同一坐标系。
//!
//! ## 密度（ADR-12 升级，参考 Android `DisplayMetrics`）
//!
//! 通过 [`crate::DisplayMetrics`] 驱动：文本字号（sp）乘 `scaled_density`，圆角 /
//! 描边 / 内边距（dp）乘 `density`；最终像素**取整**（Android
//! `complexToDimensionPixelSize` 语义）。

use peniko::Color;

use crate::platform::DisplayMetrics;
use crate::view::{EdgeInsets, Rect, View, WidgetKind, WidgetView};

/// 一条绘制指令。
///
/// 不派生 `PartialEq`：`peniko::Color` 无 `PartialEq`（T4 实证），比对请用
/// `Color::to_rgba8()`。
#[derive(Clone, Debug)]
pub enum DrawCommand {
    /// 圆角矩形填充（节点背景 / 卡片 / 图片占位 / 进度条等）；`corner_radius` 为 0 时即矩形。
    FillRect {
        /// 布局产出的绝对像素矩形。
        rect: Rect,
        /// 填充色。
        color: Color,
        /// 圆角半径（物理像素）。
        corner_radius: f32,
    },
    /// 圆角矩形描边（边框：CardView 轮廓 / EditText 输入边框 / CheckBox 方框 / Switch 轨道）。
    StrokeRect {
        /// 绝对像素矩形。
        rect: Rect,
        /// 描边色。
        color: Color,
        /// 线宽（物理像素）。
        width_px: f32,
        /// 圆角半径（物理像素）。
        corner_radius: f32,
    },
    /// 单行文本：左对齐于 `rect.x`，垂直居中于 `rect`（基线由渲染器按字体
    /// 度量确定，见 [`centered_baseline`）。`rect` 为内缩后的内容区。
    Text {
        /// 文本所在的矩形（决定左边界与垂直居中区间）。
        rect: Rect,
        /// 文本内容（v1 单行，不换行）。
        text: String,
        /// 字号（**物理像素**，已乘 scaled_density）。
        size_px: f32,
        /// 文字颜色。
        color: Color,
    },
}

/// sp → 物理像素（ADR-12：sp 乘 scaled_density，含字体缩放；与布局阶段同一规则）。
///
/// `scale` 即 `DisplayMetrics.scaled_density`。density 被夹到非负，避免异常配置下
/// 把字号算成负数。
pub fn sp_to_px(sp: f32, scale: f32) -> f32 {
    sp * scale.max(0.0)
}

/// 前序遍历视图树，生成绘制指令序列。
///
/// 顺序契约（画家算法）：
///
/// 1. 节点自身的背景 / 阴影先于其文本、也先于其子节点——背景在下层；
/// 2. 子节点按添加顺序产出——后添加者画在上层（与 hit-test 的逆序探测对称，
///    「看得见的先被点到」）。
///
/// 零面积（`width <= 0` 或 `height <= 0`）节点的**整棵子树**被跳过：子节点受
/// 父容器约束，父不可见时子必然不可见；同时避免给 kurbo 造成退化形状。
pub fn build_draw_list<Msg>(root: &View<Msg>, density: f32) -> Vec<DrawCommand> {
    build_draw_list_with(root, &DisplayMetrics::new(0.0, 0.0, density))
}

/// 前序遍历视图树，以 [`DisplayMetrics`] 驱动密度换算（推荐入口）。
pub fn build_draw_list_with<Msg>(root: &View<Msg>, metrics: &DisplayMetrics) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    collect(root, metrics, &mut commands);
    commands
}

fn collect<Msg>(node: &View<Msg>, metrics: &DisplayMetrics, out: &mut Vec<DrawCommand>) {
    match node {
        View::TextView(tv) => {
            let rect = tv.computed_rect;
            if rect.width <= 0.0 || rect.height <= 0.0 {
                return;
            }
            draw_background(rect, tv.background, tv.stroke, metrics, out);
            if !tv.text.is_empty() {
                let inner = inner_rect(rect, tv.padding, metrics);
                out.push(DrawCommand::Text {
                    rect: inner,
                    text: tv.text.clone(),
                    size_px: sp_to_px(tv.text_size, metrics.scaled_density),
                    color: tv.text_color,
                });
            }
        }
        View::ViewGroup(group) => {
            let rect = group.computed_rect;
            if rect.width <= 0.0 || rect.height <= 0.0 {
                return;
            }
            draw_background(rect, group.background, group.stroke, metrics, out);
            for child in &group.children {
                collect(child, metrics, out);
            }
        }
        View::Widget(w) => collect_widget(w, metrics, out),
    }
}

/// 绘制复合组件（按 `WidgetKind` 派发）。
fn collect_widget<Msg>(w: &WidgetView<Msg>, metrics: &DisplayMetrics, out: &mut Vec<DrawCommand>) {
    let rect = w.common.computed_rect;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let density = metrics.density;
    let corner_px = metrics.round_px(w.common.corner_radius_dp * density);

    match &w.kind {
        WidgetKind::Card(card) => {
            // 阴影（elevation）画在背景之下，模拟 Android CardView 的抬升感。
            if card.elevation_dp > 0.0 {
                let e = metrics.round_px(card.elevation_dp * density);
                let alpha = (card.elevation_dp * 25.0).clamp(0.0, 90.0) as u8;
                let shadow = Rect {
                    x: rect.x,
                    y: rect.y + e,
                    width: rect.width,
                    height: rect.height,
                };
                out.push(DrawCommand::FillRect {
                    rect: shadow,
                    color: Color::from_rgba8(0x00, 0x00, 0x00, alpha),
                    corner_radius: corner_px,
                });
            }
            draw_background(rect, w.common.background, w.common.stroke, metrics, out);
            for child in &card.children {
                collect(child, metrics, out);
            }
        }
        WidgetKind::Button(b) => {
            draw_background(rect, w.common.background, w.common.stroke, metrics, out);
            let inner = inner_rect(rect, w.common.padding, metrics);
            out.push(DrawCommand::Text {
                rect: inner,
                text: b.text.clone(),
                size_px: sp_to_px(b.text_size, metrics.scaled_density),
                color: b.text_color,
            });
        }
        WidgetKind::Edit(e) => {
            draw_background(rect, w.common.background, w.common.stroke, metrics, out);
            let inner = inner_rect(rect, w.common.padding, metrics);
            let (text, color) = if e.text.is_empty() {
                (e.hint.clone(), e.hint_color)
            } else {
                (e.text.clone(), e.text_color)
            };
            if !text.is_empty() {
                out.push(DrawCommand::Text {
                    rect: inner,
                    text,
                    size_px: sp_to_px(e.text_size, metrics.scaled_density),
                    color,
                });
            }
        }
        WidgetKind::Image(img) => {
            // v1 仅占位：用占位色填充（真实图片解码留待后续）。
            out.push(DrawCommand::FillRect {
                rect,
                color: img.placeholder,
                corner_radius: corner_px,
            });
            if let Some(c) = w.common.stroke.color {
                out.push(DrawCommand::StrokeRect {
                    rect,
                    color: c,
                    width_px: metrics.round_px(w.common.stroke.width_dp * density),
                    corner_radius: corner_px,
                });
            }
        }
        WidgetKind::Progress(p) => {
            // 轨道。
            out.push(DrawCommand::FillRect {
                rect,
                color: p.track_color,
                corner_radius: match p.orientation {
                    crate::view::ProgressOrientation::Horizontal => corner_px,
                    crate::view::ProgressOrientation::Circular => rect.height / 2.0,
                },
            });
            let prog = p.progress.clamp(0.0, 1.0);
            if prog > 0.0 {
                let (frect, radius) = match p.orientation {
                    crate::view::ProgressOrientation::Horizontal => (
                        Rect {
                            x: rect.x,
                            y: rect.y,
                            width: (rect.width * prog).max(0.0),
                            height: rect.height,
                        },
                        corner_px,
                    ),
                    crate::view::ProgressOrientation::Circular => {
                        // 环形近似：居中的实心圆，直径随进度缩放（v1 占位）。
                        let s = (rect.width * prog).max(0.0);
                        (
                            Rect {
                                x: rect.x + (rect.width - s) / 2.0,
                                y: rect.y + (rect.height - s) / 2.0,
                                width: s,
                                height: s,
                            },
                            s / 2.0,
                        )
                    }
                };
                out.push(DrawCommand::FillRect {
                    rect: frect,
                    color: p.progress_color,
                    corner_radius: radius,
                });
            }
        }
        WidgetKind::Check(c) => {
            let stroke_w = metrics.round_px(2.0 * density);
            out.push(DrawCommand::StrokeRect {
                rect,
                color: c.box_color,
                width_px: stroke_w,
                corner_radius: metrics.round_px(2.0 * density),
            });
            if c.checked {
                out.push(DrawCommand::Text {
                    rect,
                    text: "✓".to_string(),
                    size_px: metrics.round_px(c.size_dp * density * 0.8),
                    color: c.check_color,
                });
            }
        }
        WidgetKind::Switch(s) => {
            let radius = rect.height / 2.0;
            out.push(DrawCommand::FillRect {
                rect,
                color: if s.checked { s.on_color } else { s.off_color },
                corner_radius: radius,
            });
            // 滑块：在轨道内居中、按开关态左右贴边。
            let thumb = (rect.height - 4.0).max(0.0);
            let tx = if s.checked {
                rect.x + rect.width - rect.height / 2.0 - thumb / 2.0
            } else {
                rect.x + rect.height / 2.0 - thumb / 2.0
            };
            out.push(DrawCommand::FillRect {
                rect: Rect {
                    x: tx,
                    y: rect.y + (rect.height - thumb) / 2.0,
                    width: thumb,
                    height: thumb,
                },
                color: s.thumb_color,
                corner_radius: thumb / 2.0,
            });
        }
        WidgetKind::Space(_) => {
            // 纯占位间隔，不绘制任何内容。
        }
    }
}

/// 绘制节点的背景填充与描边（圆角半径对 Widget 用 dp 换算后的像素值）。
fn draw_background(
    rect: Rect,
    background: crate::view::Background,
    stroke: crate::view::Stroke,
    metrics: &DisplayMetrics,
    out: &mut Vec<DrawCommand>,
) {
    if let Some(color) = background.color {
        out.push(DrawCommand::FillRect {
            rect,
            color,
            corner_radius: background.corner_radius,
        });
    }
    if let Some(color) = stroke.color {
        out.push(DrawCommand::StrokeRect {
            rect,
            color,
            width_px: metrics.round_px(stroke.width_dp * metrics.density),
            corner_radius: background.corner_radius,
        });
    }
}

/// 由节点矩形扣去内边距，得到内容绘制区（文本 / 子内容在此内缩）。
fn inner_rect(rect: Rect, padding: EdgeInsets, metrics: &DisplayMetrics) -> Rect {
    let p = pad_px(padding, metrics);
    Rect {
        x: rect.x + p.left,
        y: rect.y + p.top,
        width: (rect.width - p.left - p.right).max(0.0),
        height: (rect.height - p.top - p.bottom).max(0.0),
    }
}

/// 四向 padding 从 dp 换算为物理像素（与 margin 同一规则，ADR-12）。
fn pad_px(padding: EdgeInsets, metrics: &DisplayMetrics) -> EdgeInsets {
    EdgeInsets {
        left: metrics.round_px(padding.left * metrics.density),
        top: metrics.round_px(padding.top * metrics.density),
        right: metrics.round_px(padding.right * metrics.density),
        bottom: metrics.round_px(padding.bottom * metrics.density),
    }
}

/// 在高度为 `height` 的矩形内垂直居中一行文本时，基线相对矩形**顶部**的偏移。
///
/// `ascent` / `descent` 均取**正值**（基线上方 / 下方的距离）。skrifa 的
/// `Metrics::descent` 为负值，调用方须取绝对值后再传入（见
/// `render::font::FontCache::vertical_metrics`）。
pub fn centered_baseline(height: f32, ascent: f32, descent: f32) -> f32 {
    (height - (ascent + descent)) * 0.5 + ascent
}
