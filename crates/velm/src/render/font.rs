//! render/font.rs — 系统字体加载、极简水平排版与字体缓存（ADR-06：skrifa 直绘）。
//!
//! vello_gpu 只提供 glyph run 编码（`Scene::glyph_run().fill_glyphs()`，来自 glifo），
//! 不含字体解析与排版；
//! 本模块用 skrifa 0.44 完成 cmap（字符→glyph id）与水平
//! advance 累加，产出 glifo `Glyph`。不做 kerning / shaping / BiDi / 换行——
//! P0 计数器仅需中英文单行（ADR-10），复杂排版留待后续评估 glifo/parley。
//!
//! 字体直接读 Android 系统字体（`/system/fonts`，对所有进程可读），不打包进
//! APK：NotoSansCJK 约 32MB，打包会让 APK 膨胀，系统字体在 minSdk24 上稳定存在。
//!
//! 本模块依赖 skrifa/vello_gpu（android-only 依赖），故**只在 android 目标编译**；
//! 与字体无关的纯几何（基线居中）放在 `render::scene`，以便 host 单测。

use std::sync::Arc;

use skrifa::instance::{LocationRef, Size};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};
// T16 迁移：字形类型与字体字节容器改用 vello_common/glifo（与 vello_gpu 同源）。
// - `Glyph` 来自 glifo（vello_gpu 的 `glyph_run().fill_glyphs()` 要求的精确类型）。
// - `Blob`/`FontData` 来自 vello_common::peniko（与 vello_gpu 的 font 参数类型同一）。
use glifo::Glyph;
use vello_common::peniko::{Blob, FontData};

/// Android 系统 Roboto（拉丁/数字），单字体 ttf，collection index 0。
pub const ROBOTO_REGULAR: &str = "/system/fonts/Roboto-Regular.ttf";
/// Android 系统 Noto Sans CJK ttc；index 2 = SC（简体中文，实测见 render-poc §4）。
pub const NOTO_SANS_CJK_SC: (&str, u32) = ("/system/fonts/NotoSansCJK-Regular.ttc", 2);

/// 一个已加载的字体：字节常驻（供 skrifa 解析与 vello FontData 共享）。
pub struct FontFace {
    bytes: Arc<Vec<u8>>,
    index: u32,
    /// 传给 `Scene::glyph_run` 的字体数据（与 `bytes` 共享同一分配）。
    pub data: FontData,
    /// name 表 family name（诊断用，加载日志之外保留以便后续排版调试）。
    #[allow(dead_code)]
    pub family: String,
}

impl FontFace {
    /// 从系统路径加载字体；`index` 为 ttc/otc 子表序号（普通 ttf 传 0）。
    pub fn load(path: &str, index: u32) -> Option<Self> {
        let bytes = Arc::new(std::fs::read(path).ok()?);
        let font = FontRef::from_index(&bytes, index).ok()?;
        let family = font
            .localized_strings(StringId::FAMILY_NAME)
            .english_or_first()
            .map(|s| s.chars().collect())
            .unwrap_or_default();
        let data = FontData::new(Blob::new(bytes.clone()), index);
        log::info!(
            "已加载字体 {family}（{path}#{index}，{} 字节）",
            bytes.len()
        );
        Some(Self {
            bytes,
            index,
            data,
            family,
        })
    }

    /// 逐字符 cmap + 水平 advance，布局成一行 glifo glyph（y=0，x 为相对
    /// run 原点的像素偏移；run 的基线位置由 `glyph_run().set_transform()` 给）。
    /// 返回 `(glyphs, 行宽 px)`。字体缺失的字符被跳过（调用方应先用
    /// `contains` 做字体回退分段）。
    pub fn shape(&self, text: &str, px: f32) -> (Vec<Glyph>, f32) {
        let Ok(font) = FontRef::from_index(&self.bytes, self.index) else {
            return (Vec::new(), 0.0);
        };
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(Size::new(px), LocationRef::default());
        let mut pen_x = 0.0f32;
        let mut glyphs = Vec::new();
        for ch in text.chars() {
            let Some(gid) = charmap.map(ch) else {
                continue;
            };
            glyphs.push(Glyph {
                id: gid.to_u32(),
                x: pen_x,
                y: 0.0,
            });
            if let Some(advance) = metrics.advance_width(gid) {
                pen_x += advance;
            }
        }
        (glyphs, pen_x)
    }

    /// 该字体是否包含字符的 glyph（用于字体回退）。
    pub fn contains(&self, ch: char) -> bool {
        FontRef::from_index(&self.bytes, self.index)
            .ok()
            .and_then(|font| font.charmap().map(ch))
            .is_some()
    }

    /// 垂直度量 `(ascent, descent)`，单位像素，均返回**正值**（基线上方 / 下方）。
    ///
    /// skrifa 的 `Metrics::descent` 为负值，这里取绝对值归一，供
    /// `scene::centered_baseline` 使用。
    pub fn vertical_metrics(&self, px: f32) -> Option<(f32, f32)> {
        let font = FontRef::from_index(&self.bytes, self.index).ok()?;
        let metrics = font.metrics(Size::new(px), LocationRef::default());
        Some((metrics.ascent, metrics.descent.abs()))
    }
}

/// 字体缺失时的经验垂直度量比例（ascent 0.8em / descent 0.2em）。
///
/// 只影响文字的垂直居中位置，不会导致缺字——缺字是 `shape_line` 返回空 run
/// 的结果，两者独立。
const FALLBACK_ASCENT_RATIO: f32 = 0.8;
const FALLBACK_DESCENT_RATIO: f32 = 0.2;

/// 一行中属于同一字体的连续 glyph 段（字体回退的产物）。
pub struct GlyphRun<'a> {
    pub face: &'a FontFace,
    /// glyph 的 x 已换算为相对整行原点的偏移。
    pub glyphs: Vec<Glyph>,
}

/// 字体缓存：进程内只加载一次，渲染器持有。
pub struct FontCache {
    /// Roboto（拉丁/数字）；加载失败则该类字符不出字。
    pub roboto: Option<FontFace>,
    /// Noto Sans CJK SC（中文回退，ttc index 2）。
    pub noto_sc: Option<FontFace>,
}

impl FontCache {
    /// 加载系统字体；任一字体失败只告警，不阻断图形渲染（ADR-11）。
    pub fn load() -> Self {
        let roboto = FontFace::load(ROBOTO_REGULAR, 0);
        if roboto.is_none() {
            log::warn!("Roboto 加载失败，拉丁文本不可用");
        }
        let (path, index) = NOTO_SANS_CJK_SC;
        let noto_sc = FontFace::load(path, index);
        if noto_sc.is_none() {
            log::warn!("NotoSansCJK 加载失败，中文文本不可用");
        }
        Self { roboto, noto_sc }
    }

    /// 把一行文本按字体覆盖范围切成多个 run：`primary` 含有的字符用 primary，
    /// 否则尝试 `fallback`，两者都不含的字符跳过。每段 glyph 的 x 累加为整行
    /// 坐标，绘制时所有 run 共用同一个行原点 transform。
    pub fn shape_line(&self, text: &str, px: f32) -> Vec<GlyphRun<'_>> {
        let Some(primary) = self.roboto.as_ref() else {
            return Vec::new();
        };
        shape_runs(text, px, primary, self.noto_sc.as_ref())
    }

    /// 当前字体的垂直度量 `(ascent, descent)`（均正值，像素）。
    ///
    /// 字体缺失时退回经验比例，保证文本仍有合理的垂直居中位置。
    pub fn vertical_metrics(&self, px: f32) -> (f32, f32) {
        self.roboto
            .as_ref()
            .or(self.noto_sc.as_ref())
            .and_then(|face| face.vertical_metrics(px))
            .unwrap_or((px * FALLBACK_ASCENT_RATIO, px * FALLBACK_DESCENT_RATIO))
    }
}

/// 按字体覆盖范围把一行文本切成多个连续 run（详见 [`FontCache::shape_line`]）。
pub fn shape_runs<'a>(
    text: &str,
    px: f32,
    primary: &'a FontFace,
    fallback: Option<&'a FontFace>,
) -> Vec<GlyphRun<'a>> {
    // 先切成 (face, 子串) 连续段。
    let mut segments: Vec<(&FontFace, String)> = Vec::new();
    for ch in text.chars() {
        let face = if primary.contains(ch) {
            primary
        } else if let Some(f) = fallback.filter(|f| f.contains(ch)) {
            f
        } else {
            continue;
        };
        match segments.last_mut() {
            Some(last) if std::ptr::addr_eq(last.0, face) => last.1.push(ch),
            _ => segments.push((face, ch.to_string())),
        }
    }

    let mut runs = Vec::new();
    let mut pen_x = 0.0f32;
    for (face, sub) in &segments {
        let (mut glyphs, width) = face.shape(sub, px);
        for g in &mut glyphs {
            g.x += pen_x;
        }
        pen_x += width;
        runs.push(GlyphRun { face, glyphs });
    }
    runs
}
