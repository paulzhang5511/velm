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

use peniko::Color;

use crate::view::Rect;
use crate::view::View;

/// 一条绘制指令。
///
/// 不派生 `PartialEq`：`peniko::Color` 无 `PartialEq`（T4 实证），比对请用
/// `Color::to_rgba8()`。
#[derive(Clone, Debug)]
pub enum DrawCommand {
    /// 圆角矩形填充（节点背景）；`corner_radius` 为 0 时即矩形。
    FillRect {
        /// 布局产出的绝对像素矩形。
        rect: Rect,
        /// 填充色。
        color: Color,
        /// 圆角半径（物理像素）。
        corner_radius: f32,
    },
    /// 单行文本：左对齐于 `rect.x`，垂直居中于 `rect`（基线由渲染器按字体
    /// 度量确定，见 [`centered_baseline`]）。
    Text {
        /// 文本所在的矩形（决定左边界与垂直居中区间）。
        rect: Rect,
        /// 文本内容（v1 单行，不换行）。
        text: String,
        /// 字号（**物理像素**，已乘 density）。
        size_px: f32,
        /// 文字颜色。
        color: Color,
    },
}

/// sp → 物理像素（ADR-12：sp/dp 一律乘 density，与布局阶段同一规则）。
///
/// density 被夹到非负，避免异常配置下把字号算成负数。
pub fn sp_to_px(sp: f32, density: f32) -> f32 {
    sp * density.max(0.0)
}

/// 前序遍历视图树，生成绘制指令序列。
///
/// 顺序契约（画家算法）：
///
/// 1. 节点自身的背景先于其文本、也先于其子节点——背景在下层；
/// 2. 子节点按添加顺序产出——后添加者画在上层（与 hit-test 的逆序探测对称，
///    「看得见的先被点到」）。
///
/// 零面积（`width <= 0` 或 `height <= 0`）节点的**整棵子树**被跳过：子节点受
/// 父容器约束，父不可见时子必然不可见；同时避免给 kurbo 造成退化形状。
pub fn build_draw_list<Msg>(root: &View<Msg>, density: f32) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    collect(root, density, &mut commands);
    commands
}

fn collect<Msg>(node: &View<Msg>, density: f32, out: &mut Vec<DrawCommand>) {
    let (rect, background) = match node {
        View::TextView(tv) => (tv.computed_rect, tv.background),
        View::ViewGroup(group) => (group.computed_rect, group.background),
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    if let Some(color) = background.color {
        out.push(DrawCommand::FillRect {
            rect,
            color,
            corner_radius: background.corner_radius,
        });
    }

    match node {
        View::TextView(tv) => {
            if !tv.text.is_empty() {
                out.push(DrawCommand::Text {
                    rect,
                    text: tv.text.clone(),
                    size_px: sp_to_px(tv.text_size, density),
                    color: tv.text_color,
                });
            }
        }
        View::ViewGroup(group) => {
            for child in &group.children {
                collect(child, density, out);
            }
        }
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
