//! layout/measure.rs — 手写 LinearLayout 测量与布局（ADR-05，SPEC §7.4）。
//!
//! 两遍算法，均为 O(n)：
//! 1. `measure`：自顶向下求每个节点的物理像素宽高，写入 `computed_rect`（x/y 置 0）；
//!    容器 WrapContent 由子节点尺寸求和得到（**修正 docs 把容器 WrapContent
//!    等同 MatchParent 的缺陷**）。
//! 2. `place`：自顶向下写入绝对像素坐标，主轴游标按「子节点主轴尺寸 + 相邻 margin」累加。
//!
//! 本模块为纯 host 逻辑，不含任何 android 符号（SPEC §10.2）。

use crate::view::{EdgeInsets, LayoutDimension, Orientation, View};

/// 文本宽度估算系数：docs 公式 `chars * size * 0.6`，仅 ASCII 近似。
const TEXT_WIDTH_FACTOR: f32 = 0.6;

/// 行高系数：WrapContent 高度 = 字号（px） * 1.4。
const LINE_HEIGHT_FACTOR: f32 = 1.4;

/// 估算一行文本的物理像素宽度（SPEC §7.4 允许的 v1 近似）。
///
/// v1 用字符数近似；P1 接入 skrifa 真实字形 advance 时**只需替换本函数**，
/// 调用方无需改动。真正的 clamp 到父内容宽发生在 [`measure`] 内。
pub fn estimate_text_width(text: &str, text_size_px: f32) -> f32 {
    text.chars().count() as f32 * text_size_px * TEXT_WIDTH_FACTOR
}

/// 单行文本行高（物理像素）。
fn line_height(text_size_px: f32) -> f32 {
    text_size_px * LINE_HEIGHT_FACTOR
}

/// 入口：以屏幕物理像素宽高为根约束，递归写满每个节点的 `computed_rect`。
///
/// `density` 由平台层按 ADR-12 提供（dpi/160.0，拿不到时兜底 1.0）；Dp/sp 统一乘它换算。
pub fn measure_and_layout<Msg>(root: &mut View<Msg>, width_px: f32, height_px: f32, density: f32) {
    // 根节点与子节点同规则：可用区 = 窗口尺寸扣除自身四向 margin（v1 无 padding）。
    // margin 为 0 时即「原点 (0,0)、约束为整窗尺寸」（SPEC §7.4 第 1 条）。
    let margin = margin_of(root);
    let avail_w = (width_px - margin.left - margin.right).max(0.0);
    let avail_h = (height_px - margin.top - margin.bottom).max(0.0);

    // 先取方向再 place：避免同时对 root 做可变与不可变借用。
    let orientation = root_orientation(root);
    measure(root, avail_w, avail_h, density);
    place(root, orientation, 0.0, 0.0, 0.0);
}

/// 第一遍：求尺寸。返回该节点的物理像素宽高，并写入 `computed_rect`。
fn measure<Msg>(node: &mut View<Msg>, avail_w: f32, avail_h: f32, density: f32) -> (f32, f32) {
    let (width, height) = match node {
        View::TextView(tv) => {
            let size_px = tv.text_size * density;
            let width = match tv.layout_params.width {
                LayoutDimension::MatchParent => avail_w,
                LayoutDimension::Dp(v) => v * density,
                LayoutDimension::WrapContent => estimate_text_width(&tv.text, size_px).min(avail_w),
            };
            let height = match tv.layout_params.height {
                LayoutDimension::MatchParent => avail_h,
                LayoutDimension::Dp(v) => v * density,
                LayoutDimension::WrapContent => line_height(size_px),
            };
            (width, height)
        }
        View::ViewGroup(vg) => measure_group(vg, avail_w, avail_h, density),
    };

    // 可用区被 margin 吃光时尺寸退化为 0，不允许出现负值。
    let width = width.max(0.0);
    let height = height.max(0.0);
    set_size(node, width, height);
    (width, height)
}

/// ViewGroup 求尺寸：先测量全部子节点，再按主轴求和 / 交叉轴取最大值得到包裹尺寸。
fn measure_group<Msg>(
    group: &mut crate::view::ViewGroup<Msg>,
    avail_w: f32,
    avail_h: f32,
    density: f32,
) -> (f32, f32) {
    let orientation = group.orientation;

    let mut main_sum = 0.0f32;
    let mut cross_max = 0.0f32;

    for child in group.children.iter_mut() {
        let margin = margin_of(child);
        // 子节点可用区 = 父可用区扣除子节点自身四向 margin（v1 无 padding）。
        let child_avail_w = (avail_w - margin.left - margin.right).max(0.0);
        let child_avail_h = (avail_h - margin.top - margin.bottom).max(0.0);
        let (child_w, child_h) = measure(child, child_avail_w, child_avail_h, density);

        match orientation {
            Orientation::Vertical => {
                main_sum += child_h + margin.top + margin.bottom;
                cross_max = cross_max.max(child_w + margin.left + margin.right);
            }
            Orientation::Horizontal => {
                main_sum += child_w + margin.left + margin.right;
                cross_max = cross_max.max(child_h + margin.top + margin.bottom);
            }
        }
    }

    let width = match group.layout_params.width {
        LayoutDimension::MatchParent => avail_w,
        LayoutDimension::Dp(v) => v * density,
        LayoutDimension::WrapContent => match orientation {
            Orientation::Vertical => cross_max,
            Orientation::Horizontal => main_sum,
        },
    };
    let height = match group.layout_params.height {
        LayoutDimension::MatchParent => avail_h,
        LayoutDimension::Dp(v) => v * density,
        LayoutDimension::WrapContent => match orientation {
            Orientation::Vertical => main_sum,
            Orientation::Horizontal => cross_max,
        },
    };

    (width, height)
}

/// 第二遍：写入绝对像素坐标；`content_x/content_y` 为父节点内容区原点，
/// `main_offset` 为该节点在父主轴上的游标位置。
fn place<Msg>(
    node: &mut View<Msg>,
    parent_orientation: Orientation,
    content_x: f32,
    content_y: f32,
    main_offset: f32,
) {
    let margin = margin_of(node);

    let (x, y) = match parent_orientation {
        Orientation::Vertical => (
            content_x + margin.left,
            content_y + main_offset + margin.top,
        ),
        Orientation::Horizontal => (
            content_x + main_offset + margin.left,
            content_y + margin.top,
        ),
    };
    set_origin(node, x, y);

    if let View::ViewGroup(group) = node {
        let orientation = group.orientation;
        let origin_x = group.computed_rect.x;
        let origin_y = group.computed_rect.y;

        let mut cursor = 0.0f32;
        for child in group.children.iter_mut() {
            place(child, orientation, origin_x, origin_y, cursor);
            cursor += main_extent(child, orientation);
        }
    }
}

/// 取根节点方向（叶子节点退化为 Vertical，此时主轴游标恒为 0）。
fn root_orientation<Msg>(root: &View<Msg>) -> Orientation {
    match root {
        View::ViewGroup(group) => group.orientation,
        View::TextView(_) => Orientation::Vertical,
    }
}

/// 节点在主轴上占用的推进量 = 主轴尺寸 + 主轴两侧 margin。
fn main_extent<Msg>(node: &View<Msg>, orientation: Orientation) -> f32 {
    let (width, height) = size_of(node);
    let margin = margin_of(node);
    match orientation {
        Orientation::Vertical => height + margin.top + margin.bottom,
        Orientation::Horizontal => width + margin.left + margin.right,
    }
}

fn margin_of<Msg>(node: &View<Msg>) -> EdgeInsets {
    match node {
        View::TextView(tv) => tv.layout_params.margin,
        View::ViewGroup(vg) => vg.layout_params.margin,
    }
}

fn size_of<Msg>(node: &View<Msg>) -> (f32, f32) {
    match node {
        View::TextView(tv) => (tv.computed_rect.width, tv.computed_rect.height),
        View::ViewGroup(vg) => (vg.computed_rect.width, vg.computed_rect.height),
    }
}

fn set_origin<Msg>(node: &mut View<Msg>, x: f32, y: f32) {
    match node {
        View::TextView(tv) => {
            tv.computed_rect.x = x;
            tv.computed_rect.y = y;
        }
        View::ViewGroup(vg) => {
            vg.computed_rect.x = x;
            vg.computed_rect.y = y;
        }
    }
}

fn set_size<Msg>(node: &mut View<Msg>, width: f32, height: f32) {
    match node {
        View::TextView(tv) => {
            tv.computed_rect.width = width;
            tv.computed_rect.height = height;
        }
        View::ViewGroup(vg) => {
            vg.computed_rect.width = width;
            vg.computed_rect.height = height;
        }
    }
}
