//! 系统字体加载与极简水平排版（T2 Slice 3，ADR-06：skrifa 直绘）。
//!
//! vello 只提供 glyph run 编码（`Scene::draw_glyphs`），不含字体解析与排版；
//! 本模块用 vello 已内置的 skrifa 0.44 完成 cmap（字符→glyph id）与水平
//! advance 累加，产出 vello `Glyph`。不做 kerning / shaping / BiDi / 换行——
//! P0 计数器仅需中英文单行（ADR-10），复杂排版留待后续评估 glifo/parley。
//!
//! 字体直接读 Android 系统字体（`/system/fonts`，对所有进程可读），不打包进
//! APK：NotoSansCJK 约 32MB，打包会让 APK 膨胀，系统字体在 minSdk24 上稳定存在。

use std::sync::Arc;

use skrifa::instance::{LocationRef, Size};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};
use vello::Glyph;
use vello::peniko::{Blob, FontData};

/// Android 系统 Roboto（拉丁/数字），单字体 ttf，collection index 0。
pub const ROBOTO_REGULAR: &str = "/system/fonts/Roboto-Regular.ttf";
/// Android 系统 Noto Sans CJK ttc；index 2 = SC（简体中文，实测见 render-poc §4）。
pub const NOTO_SANS_CJK_SC: (&str, u32) = ("/system/fonts/NotoSansCJK-Regular.ttc", 2);

/// 一个已加载的字体：字节常驻（供 skrifa 解析与 vello FontData 共享）。
pub struct FontFace {
    bytes: Arc<Vec<u8>>,
    index: u32,
    /// 传给 `Scene::draw_glyphs` 的字体数据（与 `bytes` 共享同一分配）。
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

    /// 逐字符 cmap + 水平 advance，布局成一行 vello glyph（y=0，x 为相对
    /// run 原点的像素偏移；run 的基线位置由 `draw_glyphs().transform()` 给）。
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
}

/// 一行中属于同一字体的连续 glyph 段（字体回退的产物）。
pub struct GlyphRun<'a> {
    pub face: &'a FontFace,
    /// glyph 的 x 已换算为相对整行原点的偏移。
    pub glyphs: Vec<Glyph>,
}

/// 把一行文本按字体覆盖范围切成多个 run：`primary` 含有的字符用 primary，
/// 否则尝试 `fallback`，两者都不含的字符跳过。每段 glyph 的 x 累加为整行
/// 坐标，绘制时所有 run 共用同一个行原点 transform。
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
