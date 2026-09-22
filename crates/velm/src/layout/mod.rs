//! layout/ — 手写 LinearLayout 测量（SPEC §7.4）。
//!
//! 纯 host 逻辑：`measure_and_layout` 只输入「视图树 + 屏幕物理像素 + density」，
//! 输出写满各节点 `computed_rect`，可在开发机直接单测（§10.2 硬约束）。

mod measure;

pub use measure::{estimate_text_width, measure_and_layout};
