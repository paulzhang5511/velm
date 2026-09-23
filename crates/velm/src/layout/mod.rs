//! layout/ — 手写 LinearLayout 测量（SPEC §7.4）。
//!
//! 纯 host 逻辑：`measure_and_layout` / `measure_and_layout_with` 只输入「视图树 +
//! 屏幕物理像素 + 密度」，输出写满各节点 `computed_rect`，可在开发机直接单测
//! （§10.2 硬约束）。`measure_and_layout_with` 接受 [`crate::DisplayMetrics`]，
//! 正确区分 sp/dp 并做像素取整（ADR-12 升级，参考 Android `DisplayMetrics`）。

mod measure;

pub use measure::{estimate_text_width, measure_and_layout, measure_and_layout_with};
