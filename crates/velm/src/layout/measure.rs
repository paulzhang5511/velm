//! layout/measure.rs — 手写 LinearLayout 测量与布局（ADR-05，SPEC §7.4）。
//!
//! 两遍算法，均为 O(n)：
//! 1. `measure`：自顶向下求每个节点的物理像素宽高，写入 `computed_rect`（x/y 置 0）；
//!    容器 WrapContent 由子节点尺寸求和得到（**修正 docs 把容器 WrapContent
//!    等同 MatchParent 的缺陷**）。
//! 2. `place`：自顶向下写入绝对像素坐标，主轴游标按「子节点主轴尺寸 + 相邻 margin」累加。
//!
//! 本模块为纯 host 逻辑，不含任何 android 符号（SPEC §10.2）。
//!
//! ## 密度（ADR-12 升级，参考 Android `DisplayMetrics`）
//!
//! - 入口接受 [`DisplayMetrics`]：文本字号（sp）乘 `scaled_density`（含字体缩放），
//!   尺寸 / dp / margin / padding 乘 `density`；最终物理像素**四舍五入取整**
//!   （Android `complexToDimensionPixelSize` 语义），保证不同 density 下几何一致、渲染清晰。
//! - 节点内容区 = 自身可用区扣除四向 `padding`，子节点在 padding 之内布局（Android 容器语义）。

use crate::platform::DisplayMetrics;
use crate::view::{
    EdgeInsets, LayoutDimension, LayoutParams, Orientation, View, WidgetKind, WidgetView,
};

/// 文本宽度估算系数：docs 公式 `chars * size * 0.6`，仅 ASCII 近似。
const TEXT_WIDTH_FACTOR: f32 = 0.6;

/// 行高系数：WrapContent 高度 = 字号（px） * 1.4。
const LINE_HEIGHT_FACTOR: f32 = 1.4;

/// 估算一行文本的物理像素宽度（SPEC §7.4 允许的 v1 近似）。
///
/// v1 用字符数近似；P1 接入 skrifa 真实字形 advance 时**只需替换本函数**，
/// 调用方无需改动。真正的 clamp 到父内容宽发生在 [`measure_text`] 内。
pub fn estimate_text_width(text: &str, text_size_px: f32) -> f32 {
    text.chars().count() as f32 * text_size_px * TEXT_WIDTH_FACTOR
}

/// 单行文本行高（物理像素）。
fn line_height(text_size_px: f32) -> f32 {
    text_size_px * LINE_HEIGHT_FACTOR
}

/// 入口（薄包装，保留旧签名以兼容既有调用与测试）。
///
/// `density` 按 ADR-12 提供（dpi/160.0，拿不到时兜底 1.0）；`font_scale` 默认 1.0。
pub fn measure_and_layout<Msg>(root: &mut View<Msg>, width_px: f32, height_px: f32, density: f32) {
    measure_and_layout_with(root, &DisplayMetrics::new(width_px, height_px, density))
}

/// 入口（推荐）：以 [`DisplayMetrics`] 驱动完整密度换算（sp/dp 分离、像素取整）。
///
/// 以屏幕物理像素宽高为根约束，递归写满每个节点的 `computed_rect`。
pub fn measure_and_layout_with<Msg>(root: &mut View<Msg>, metrics: &DisplayMetrics) {
    // 根节点与子节点同规则：可用区 = 窗口尺寸扣除自身四向 margin（v1 无 padding 影响根）。
    let margin = margin_px(margin_of(root), metrics);
    let avail_w = (metrics.width_px - margin.left - margin.right).max(0.0);
    let avail_h = (metrics.height_px - margin.top - margin.bottom).max(0.0);

    // 先取方向再 place：避免同时对 root 做可变与不可变借用。
    let orientation = root_orientation(root);
    measure(root, avail_w, avail_h, metrics);
    place(root, orientation, 0.0, 0.0, 0.0, metrics);
}

/// 第一遍：求尺寸。返回该节点的物理像素宽高，并写入 `computed_rect`。
fn measure<Msg>(
    node: &mut View<Msg>,
    avail_w: f32,
    avail_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let (width, height) = match node {
        View::TextView(tv) => measure_text(
            &tv.text,
            tv.text_size,
            tv.padding,
            &tv.layout_params,
            avail_w,
            avail_h,
            metrics,
        ),
        View::ViewGroup(vg) => measure_group(vg, avail_w, avail_h, metrics),
        View::Widget(w) => measure_widget(w, avail_w, avail_h, metrics),
    };

    // 可用区被 margin 吃光时尺寸退化为 0，不允许出现负值。
    let width = width.max(0.0);
    let height = height.max(0.0);
    set_size(node, width, height);
    (width, height)
}

/// 文本类叶子（TextView / Button / EditText）的尺寸：文本估算 + 四向 padding。
fn measure_text(
    text: &str,
    text_size: f32,
    padding: EdgeInsets,
    lp: &LayoutParams,
    avail_w: f32,
    avail_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let pad = margin_px(padding, metrics);
    let content_avail_w = (avail_w - pad.left - pad.right).max(0.0);
    // 字号是 sp：乘 scaled_density（含字体缩放）。
    let size_px = text_size * metrics.scaled_density;
    let content_w = estimate_text_width(text, size_px).min(content_avail_w);
    let content_h = line_height(size_px);

    let total_w = resolve_size(lp.width, avail_w, content_w + pad.left + pad.right, metrics);
    let total_h = resolve_size(
        lp.height,
        avail_h,
        content_h + pad.top + pad.bottom,
        metrics,
    );
    (metrics.round_px(total_w), metrics.round_px(total_h))
}

/// ViewGroup 求尺寸：先测量全部子节点，再按主轴求和 / 交叉轴取最大值得到包裹尺寸。
fn measure_group<Msg>(
    group: &mut crate::view::ViewGroup<Msg>,
    avail_w: f32,
    avail_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let (main_sum, cross_max) = measure_children(
        &mut group.children,
        group.orientation,
        group.padding,
        avail_w,
        avail_h,
        metrics,
    );
    resolve_container_size(
        &group.layout_params,
        group.orientation,
        (main_sum, cross_max),
        (avail_w, avail_h),
        metrics,
    )
}

/// Widget（容器 / 叶子）求尺寸；Card 递归测量其子节点。
fn measure_widget<Msg>(
    w: &mut WidgetView<Msg>,
    avail_w: f32,
    avail_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let pad = margin_px(w.common.padding, metrics);
    let content_avail_w = (avail_w - pad.left - pad.right).max(0.0);
    let content_avail_h = (avail_h - pad.top - pad.bottom).max(0.0);

    let (content_w, content_h) = match &mut w.kind {
        WidgetKind::Card(card) => {
            let (main_sum, cross_max) = measure_children(
                &mut card.children,
                card.orientation,
                EdgeInsets::default(),
                content_avail_w,
                content_avail_h,
                metrics,
            );
            container_content_size(card.orientation, main_sum, cross_max)
        }
        WidgetKind::Button(b) => text_content(
            &b.text,
            b.text_size,
            content_avail_w,
            content_avail_h,
            metrics,
        ),
        WidgetKind::Edit(e) => {
            let src = if e.text.is_empty() {
                e.hint.clone()
            } else {
                e.text.clone()
            };
            text_content(&src, e.text_size, content_avail_w, content_avail_h, metrics)
        }
        WidgetKind::Image(img) => {
            let s = img.default_size_dp * metrics.density;
            (s, s)
        }
        WidgetKind::Progress(p) => progress_content(p, content_avail_w, content_avail_h, metrics),
        WidgetKind::Check(c) => {
            let s = c.size_dp * metrics.density;
            (s, s)
        }
        WidgetKind::Switch(s) => (s.width_dp * metrics.density, s.height_dp * metrics.density),
        WidgetKind::Space(s) => (s.width_dp * metrics.density, s.height_dp * metrics.density),
    };

    let total_w = resolve_size(
        w.common.layout_params.width,
        avail_w,
        content_w + pad.left + pad.right,
        metrics,
    );
    let total_h = resolve_size(
        w.common.layout_params.height,
        avail_h,
        content_h + pad.top + pad.bottom,
        metrics,
    );
    (metrics.round_px(total_w), metrics.round_px(total_h))
}

/// 测量一组子节点，返回 (主轴累计尺寸, 交叉轴最大尺寸)。
fn measure_children<Msg>(
    children: &mut [View<Msg>],
    orientation: Orientation,
    container_padding: EdgeInsets,
    avail_w: f32,
    avail_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let pad = margin_px(container_padding, metrics);
    let content_avail_w = (avail_w - pad.left - pad.right).max(0.0);
    let content_avail_h = (avail_h - pad.top - pad.bottom).max(0.0);

    let mut main_sum = 0.0f32;
    let mut cross_max = 0.0f32;
    for child in children.iter_mut() {
        let margin = margin_px(margin_of(child), metrics);
        let child_avail_w = (content_avail_w - margin.left - margin.right).max(0.0);
        let child_avail_h = (content_avail_h - margin.top - margin.bottom).max(0.0);
        let (child_w, child_h) = measure(child, child_avail_w, child_avail_h, metrics);

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
    (main_sum, cross_max)
}

/// 容器内容尺寸（不含自身 padding）：纵排 → (cross_max, main_sum)，横排相反。
fn container_content_size(orientation: Orientation, main_sum: f32, cross_max: f32) -> (f32, f32) {
    match orientation {
        Orientation::Vertical => (cross_max, main_sum),
        Orientation::Horizontal => (main_sum, cross_max),
    }
}

/// 把容器内容尺寸 + 自身 padding 解析为最终尺寸（考虑 MatchParent / Dp / WrapContent）。
fn resolve_container_size(
    lp: &LayoutParams,
    orientation: Orientation,
    content: (f32, f32),
    avail: (f32, f32),
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let (content_w, content_h) = container_content_size(orientation, content.0, content.1);
    let total_w = resolve_size(lp.width, avail.0, content_w, metrics);
    let total_h = resolve_size(lp.height, avail.1, content_h, metrics);
    (metrics.round_px(total_w), metrics.round_px(total_h))
}

/// 文本内容的尺寸（不含 padding）。
fn text_content(
    text: &str,
    text_size: f32,
    cav_w: f32,
    _cav_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    let size_px = text_size * metrics.scaled_density;
    let w = estimate_text_width(text, size_px).min(cav_w);
    let h = line_height(size_px);
    (w, h)
}

/// 进度条内容尺寸（不含 padding）。
fn progress_content(
    p: &crate::view::ProgressSpec,
    cav_w: f32,
    _cav_h: f32,
    metrics: &DisplayMetrics,
) -> (f32, f32) {
    match p.orientation {
        crate::view::ProgressOrientation::Horizontal => {
            (cav_w, (p.thickness_dp * metrics.density).max(0.0))
        }
        crate::view::ProgressOrientation::Circular => {
            let s = p.size_dp * metrics.density;
            (s, s)
        }
    }
}

/// 按 `LayoutDimension` 解析最终尺寸：MatchParent→可用区，Dp→乘 density，WrapContent→内容尺寸。
fn resolve_size(spec: LayoutDimension, avail: f32, content: f32, metrics: &DisplayMetrics) -> f32 {
    match spec {
        LayoutDimension::MatchParent => avail,
        LayoutDimension::Dp(v) => v * metrics.density,
        LayoutDimension::WrapContent => content,
    }
    .max(0.0)
}

/// 第二遍：写入绝对像素坐标；`content_x/content_y` 为父节点内容区原点，
/// `main_offset` 为该节点在父主轴上的游标位置。
fn place<Msg>(
    node: &mut View<Msg>,
    parent_orientation: Orientation,
    content_x: f32,
    content_y: f32,
    main_offset: f32,
    metrics: &DisplayMetrics,
) {
    let margin = margin_px(margin_of(node), metrics);

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
    set_origin(node, metrics.round_px(x), metrics.round_px(y));

    // 容器：子节点布局在「自身内容区（rect 扣除 padding）」之内。
    if let View::ViewGroup(group) = node {
        let orient = group.orientation;
        let ox = group.computed_rect.x + margin_px(group.padding, metrics).left;
        let oy = group.computed_rect.y + margin_px(group.padding, metrics).top;
        let mut cursor = 0.0f32;
        for child in group.children.iter_mut() {
            place(child, orient, ox, oy, cursor, metrics);
            cursor += main_extent(child, orient, metrics);
        }
    } else if let View::Widget(w) = node
        && let WidgetKind::Card(card) = &mut w.kind
    {
        let orient = card.orientation;
        let ox = w.common.computed_rect.x + margin_px(w.common.padding, metrics).left;
        let oy = w.common.computed_rect.y + margin_px(w.common.padding, metrics).top;
        let mut cursor = 0.0f32;
        for child in card.children.iter_mut() {
            place(child, orient, ox, oy, cursor, metrics);
            cursor += main_extent(child, orient, metrics);
        }
    }
}

/// 取根节点方向（叶子节点退化为 Vertical，此时主轴游标恒为 0）。
fn root_orientation<Msg>(root: &View<Msg>) -> Orientation {
    match root {
        View::ViewGroup(group) => group.orientation,
        View::Widget(w) => match &w.kind {
            WidgetKind::Card(card) => card.orientation,
            _ => Orientation::Vertical,
        },
        View::TextView(_) => Orientation::Vertical,
    }
}

/// 节点在主轴上占用的推进量 = 主轴尺寸 + 主轴两侧 margin。
fn main_extent<Msg>(node: &View<Msg>, orientation: Orientation, metrics: &DisplayMetrics) -> f32 {
    let (width, height) = size_of(node);
    let margin = margin_px(margin_of(node), metrics);
    match orientation {
        Orientation::Vertical => height + margin.top + margin.bottom,
        Orientation::Horizontal => width + margin.left + margin.right,
    }
}

/// 四向 margin/padding 从 dp 换算为物理像素（ADR-12：与 Dp/sp 同一套换算）。
///
/// margin / padding 声明在 dp 上，但位置偏移、可用区扣减、主轴推进量全部发生在物理
/// 像素空间，因此每一处使用 margin/padding 前都必须先乘 density——漏乘会让
/// density=2 的设备上外边距只有应有值的一半（T7 端到端用例暴露的缺陷）。
fn margin_px(margin: EdgeInsets, metrics: &DisplayMetrics) -> EdgeInsets {
    EdgeInsets {
        left: metrics.round_px(margin.left * metrics.density),
        top: metrics.round_px(margin.top * metrics.density),
        right: metrics.round_px(margin.right * metrics.density),
        bottom: metrics.round_px(margin.bottom * metrics.density),
    }
}

fn margin_of<Msg>(node: &View<Msg>) -> EdgeInsets {
    match node {
        View::TextView(tv) => tv.layout_params.margin,
        View::ViewGroup(vg) => vg.layout_params.margin,
        View::Widget(w) => w.common.layout_params.margin,
    }
}

fn size_of<Msg>(node: &View<Msg>) -> (f32, f32) {
    match node {
        View::TextView(tv) => (tv.computed_rect.width, tv.computed_rect.height),
        View::ViewGroup(vg) => (vg.computed_rect.width, vg.computed_rect.height),
        View::Widget(w) => (w.common.computed_rect.width, w.common.computed_rect.height),
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
        View::Widget(w) => {
            w.common.computed_rect.x = x;
            w.common.computed_rect.y = y;
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
        View::Widget(w) => {
            w.common.computed_rect.width = width;
            w.common.computed_rect.height = height;
        }
    }
}
