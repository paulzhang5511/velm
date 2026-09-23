//! render — 绘制指令生成（host 可测）与 vello_gpu/wgpu 渲染（android-only）。
//!
//! 与 `engine` 同样的拆分原则（SPEC §10.2）：**纯逻辑 host 可见、FFI/GPU 相关
//! 按目标 gate**，否则渲染层在 host 上整块不可测。
//!
//! - `scene`：视图树 → [`DrawCommand`] 序列（纯逻辑，含 sp→px 与基线居中几何）；
//! - `font`：系统字体加载与 skrifa 排版（android-only，依赖 skrifa/vello_gpu）；
//! - `vello_renderer`：wgpu 30 surface + vello_gpu 0.2 编码（android-only）。

pub mod scene;

#[cfg(target_os = "android")]
pub mod font;
#[cfg(target_os = "android")]
pub mod vello_renderer;

pub use scene::{DrawCommand, build_draw_list, build_draw_list_with, centered_baseline, sp_to_px};

#[cfg(target_os = "android")]
pub use vello_renderer::VelloRenderer;
