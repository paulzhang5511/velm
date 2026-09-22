//! render/vello_renderer.rs — vello_gpu 0.2 / wgpu 30 渲染器（SPEC §7.8，T16 迁移）。
//!
//! 链路：`NativeWindowWrapper`（rwh 0.6 句柄）→ wgpu 30 unsafe surface（Vulkan）
//! → adapter/device → vello_gpu `Renderer` + `Resources` → `Scene` 直接编码到
//! surface 纹理（Sparse Strips）→ present。
//!
//! 与 vello 0.10（compute 架构）的关键差异：
//! - `Renderer::new` 返回 `(Renderer, Resources)`，二者均为持久资源，后者持有字形图集；
//! - 没有中间 Rgba8Unorm 纹理 + `TextureBlitter`：vello_gpu 直接渲染到 surface 视图；
//! - `Scene` 是「状态机式」录制器：`set_paint` 设色、`fill_rect`/`fill_path`/`glyph_run`
//!   各按当前 paint/transform 录制；
//! - 需要一张 Depth24Plus 深度纹理做 overdraw 裁剪；清屏通过 `TargetInit::Clear` 完成。
//!
//! 本模块依赖 wgpu/vello_gpu/vello_common/glifo/skrifa（android-only），**只在 android
//! 目标编译**；「画什么」由 `render::scene`（host 可测）决定，本模块只负责「怎么画」。

use raw_window_handle::HasDisplayHandle;
use raw_window_handle::HasWindowHandle;
use vello_common::color::{AlphaColor, Srgb};
use vello_common::kurbo::{Affine, Rect, RoundedRect, Shape};
use vello_common::peniko::Color;
use vello_gpu::{
    ClearSettings, Renderer, RenderSize, RenderTargetConfig, Resources, Scene, TargetInit,
    TextureBindings,
};
use wgpu;

use crate::error::{Error, Result};
use crate::platform::window::NativeWindowWrapper;
use crate::render::font::{FontCache, GlyphRun};
use crate::render::scene::{DrawCommand, centered_baseline};
use crate::view::View;

/// ADR-10 背景色 #121212（sRGB 直通：surface 用非 sRGB 的 Rgba8Unorm，vello_gpu
/// 的清屏管线直接写入 sRGB 编码字节值，与 SPEC §7.8 约定一致）。
const BACKGROUND: AlphaColor<Srgb> = Color::from_rgba8(0x12, 0x12, 0x12, 0xFF);

/// vello_gpu 渲染器：持有 wgpu 表面/设备、vello_gpu 渲染器与持久资源、深度纹理与字体缓存。
pub struct VelloRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// vello_gpu 渲染器（持有 shader 管线与调度状态）。
    renderer: Renderer,
    /// vello_gpu 持久资源（含字形图集），每帧 `render` 时以 `&mut` 借用。
    resources: Resources,
    /// 深度纹理视图（Depth24Plus，尺寸 = surface 尺寸），vello_gpu 用它做 overdraw 裁剪。
    depth_view: wgpu::TextureView,
    fonts: FontCache,
    /// 屏幕密度（sp/dp → 物理像素）；构造时确定，随窗口配置更新。
    density: f32,
    /// 上一帧的绘制指令：surface 重配 / 尺寸变化后用它重画，避免黑屏。
    last_frame: Vec<DrawCommand>,
}

impl VelloRenderer {
    /// 在引擎线程调用；阻塞完成 wgpu 设备请求（`pollster`）。
    ///
    /// # Safety（生命周期，由 §3.3 销毁同步协议保证）
    ///
    /// `window` 必须在渲染器存活期间保持有效：句柄以 raw 形式交给 wgpu，
    /// surface 不持有窗口引用。引擎的顺序是「先 drop 渲染器、再 release
    /// 窗口」，且主线程在销毁回调里等引擎 ack，故该顺序必然成立。
    pub fn new(
        window: &NativeWindowWrapper,
        width: u32,
        height: u32,
        density: f32,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidWindowSize(width, height));
        }
        // ADR-10：v1 唯一缓冲格式 RGBA_8888。失败即无可用 surface，不 panic。
        window
            .configure_buffers(width as i32, height as i32)
            .map_err(Error::BufferGeometry)?;

        // rwh 0.6 句柄由 T9 的包装器导出——这是「rwh 句柄能被 wgpu 接受」的
        // 唯一接线点（T9 遗留验收项在此闭合）。
        let raw_window_handle = window
            .window_handle()
            .map_err(|_| Error::WindowHandleUnavailable)?
            .as_raw();
        let raw_display_handle = window
            .display_handle()
            .map_err(|_| Error::WindowHandleUnavailable)?
            .as_raw();

        pollster::block_on(Self::init(
            raw_window_handle,
            raw_display_handle,
            width,
            height,
            density,
        ))
    }

    async fn init(
        raw_window_handle: raw_window_handle::RawWindowHandle,
        raw_display_handle: raw_window_handle::RawDisplayHandle,
        width: u32,
        height: u32,
        density: f32,
    ) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        // 模拟器 ranchu→SwiftShader 虽声明 VK_EXT_debug_utils，其
        // vkSetDebugUtilsObjectNameEXT 实现有缺陷（设备创建期段错误）；
        // 丢弃 HAL 层 debug 名绕过，真机无此问题，统一关闭无功能损失。
        descriptor.flags = wgpu::InstanceFlags::DISCARD_HAL_LABELS;
        let instance = wgpu::Instance::new(descriptor);

        // SAFETY: 句柄来自 NativeWindowWrapper（非空 + 持有 acquire 引用），
        // 且窗口按 §3.3 保证在渲染器 drop 之前不被释放。
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .map_err(|e| Error::CreateSurface(e.to_string()))?
        };

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(e) => {
                log::error!("无可用 GPU 适配器（backends=VULKAN）: {e}");
                return Err(Error::NoAdapter);
            }
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("velm-device"),
                required_features: wgpu::Features::empty(),
                // vello_gpu 管线需要完整 default limits（SwiftShader Vulkan1.3 支持）。
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| Error::RequestDevice(e.to_string()))?;

        let caps = surface.get_capabilities(&adapter);
        log::info!(
            "wgpu adapter={:?} formats={:?} present_modes={:?} alpha={:?}",
            adapter.get_info().name,
            caps.formats,
            caps.present_modes,
            caps.alpha_modes
        );

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or(Error::NoSurfaceConfig)?;
        // vello_gpu 的 fine pass 输出已是 sRGB 编码字节值（约定 target 为线性
        // Rgba8Unorm，内部完成线性混合与 sRGB 编码）。surface 必须用**非 sRGB**
        // Rgba8Unorm，blit 时值直通；若用 Rgba8UnormSrgb 会被硬件二次编码
        // 导致整体泛白（#121212→#4a4a4a）。Rgba8Unorm 在 Vulkan 上保证支持。
        config.format = wgpu::TextureFormat::Rgba8Unorm;
        // Fifo 是唯一保证支持的呈现模式（ADR-09）。
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let render_size = render_size_of(width, height);
        // vello_gpu：`Renderer::new` 返回 `(Renderer, Resources)`，Resources 是持久资源
        // （含字形图集），必须长期持有并在每帧 `render` 时以 `&mut` 传入。
        let (renderer, resources) = Renderer::new(
            &device,
            &RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: render_size.width,
                height: render_size.height,
            },
        );
        // 深度纹理：vello_gpu 用 Depth24Plus 做 overdraw 裁剪；尺寸须与 surface 一致。
        let depth_view = Renderer::create_depth_texture_view(&device, &render_size);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
            resources,
            depth_view,
            fonts: FontCache::load(),
            density,
            last_frame: Vec::new(),
        })
    }

    /// 当前 surface 尺寸（物理像素）。
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// 窗口尺寸变化：重配 swapchain 与深度纹理，并用上一帧的指令重画
    /// （否则重配后到下一帧之间会露出未定义内容）。
    ///
    /// 尺寸未变则直接返回，避免无谓的 reconfigure。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        // 深度纹理尺寸须与 surface 同步，否则 render pass 的附件 extent 校验失败。
        self.depth_view = Renderer::create_depth_texture_view(&self.device, &render_size_of(width, height));
        // 借用 last_frame 后再交给不需要 &mut self 的编码路径，故先取出。
        let commands = std::mem::take(&mut self.last_frame);
        if !commands.is_empty() {
            self.present(&commands);
            self.last_frame = commands;
        }
    }

    /// 渲染一帧：视图树 → 绘制指令 → vello_gpu 编码 → present。
    ///
    /// 失败只记日志并跳过本帧，不返回错误、不 panic（ADR-11）；surface 丢失
    /// 时由引擎重建窗口与渲染器（T12）。
    pub fn render<Msg>(&mut self, root: &View<Msg>) {
        let commands = crate::render::scene::build_draw_list(root, self.density);
        self.present(&commands);
        self.last_frame = commands;
    }

    /// 编码并 present 一帧；任何失败都只告警（ADR-11）。
    fn present(&mut self, commands: &[DrawCommand]) {
        let Some(frame) = self.acquire_frame() else {
            return;
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let (width, height) = (self.config.width, self.config.height);
        let render_size = render_size_of(width, height);
        // vello_gpu 直接渲染到 surface 视图（无中间纹理、无 blitter）。
        let mut scene = Scene::new(render_size.width, render_size.height);

        self.encode(&mut scene, commands);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("velm-render"),
            });
        if let Err(e) = self.renderer.render(
            &scene,
            &mut self.resources,
            &self.device,
            &self.queue,
            &mut encoder,
            &render_size,
            &frame_view,
            // 启用深度缓冲以减少 overdraw。
            Some(&self.depth_view),
            // 本应用不使用纹理绑定，传空即可。
            &TextureBindings::new(),
            // 清屏到 ADR-10 背景色；vello_gpu 的清屏管线写入该 sRGB 颜色。
            TargetInit::Clear(ClearSettings::Viewport { color: BACKGROUND }),
        ) {
            log::warn!("vello_gpu render 失败: {e}");
            return;
        }

        self.queue.submit([encoder.finish()]);
        // wgpu 30：present 已从 `SurfaceTexture::present()` 迁移到 `Queue::present(frame)`。
        self.queue.present(frame);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// 取一帧交换链图像；`Outdated` 重配后重试一次，其余失败返回 `None`。
    fn acquire_frame(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some(texture),
            // 窗口尺寸/格式已变：重配后重试一次（SPEC §7.8 的恢复路径）。
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(texture)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some(texture),
                    other => {
                        log::warn!("重配后仍取不到交换链图像，跳过本帧: {other:?}");
                        None
                    }
                }
            }
            // surface 丢失：只能重建窗口与渲染器（T12 的窗口重建路径）。
            wgpu::CurrentSurfaceTexture::Lost => {
                log::error!("surface 已丢失，等待窗口重建（本帧丢弃）");
                None
            }
            // Timeout / Occluded：跳过本帧，下一帧再试。
            other => {
                log::warn!("获取交换链图像失败，跳过本帧: {other:?}");
                None
            }
        }
    }

    /// 把绘制指令编码进 vello_gpu `Scene`（顺序即画家算法）。
    ///
    /// `Scene` 是状态机式录制器：先用 `set_transform`/`set_paint` 设好当前变换与颜色，
    /// 再调 `fill_rect`/`fill_path`/`glyph_run` 录制；每个 draw 前都重设变换与颜色，
    /// 避免上一 draw 的状态泄漏。
    fn encode(&mut self, scene: &mut Scene, commands: &[DrawCommand]) {
        for command in commands {
            match command {
                DrawCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } => {
                    let rect = Rect::new(
                        rect.x as f64,
                        rect.y as f64,
                        (rect.x + rect.width) as f64,
                        (rect.y + rect.height) as f64,
                    );
                    // 矩形用绝对坐标绘制，故变换重置为单位矩阵。
                    scene.set_transform(Affine::IDENTITY);
                    scene.set_paint(*color);
                    if *corner_radius > 0.0 {
                        let rounded = RoundedRect::from_rect(rect, *corner_radius as f64);
                        scene.fill_path(&rounded.to_path(0.1));
                    } else {
                        scene.fill_rect(&rect);
                    }
                }
                DrawCommand::Text {
                    rect,
                    text,
                    size_px,
                    color,
                } => {
                    let (ascent, descent) = self.fonts.vertical_metrics(*size_px);
                    let baseline = rect.y + centered_baseline(rect.height, ascent, descent);
                    // 字形以 glyph.x 作为相对行原点的偏移；用场景变换把原点平移到
                    // (rect.x, baseline)，glyph_run 会按当前变换录制（与 vello 0.10 的
                    // `.transform(translate(...))` 等价）。
                    scene.set_transform(Affine::translate((rect.x as f64, baseline as f64)));
                    scene.set_paint(*color);
                    for run in self.fonts.shape_line(text, *size_px) {
                        // 解构 run：把 glyphs 移出、face 留作 `&FontData` 引用，
                        // 二者来自不同字段，可同时满足「移动 glyphs」与「借用 face.data」。
                        let GlyphRun { face, glyphs } = run;
                        scene
                            .glyph_run(&mut self.resources, &face.data)
                            .font_size(*size_px)
                            .fill_glyphs(glyphs.into_iter());
                    }
                }
            }
        }
    }
}

/// 把 wgpu 的 u32 surface 尺寸钳制为 vello_gpu 要求的 `u16`（Android 分辨率远低于
/// u16::MAX，钳制仅为防御性）。
fn render_size_of(width: u32, height: u32) -> RenderSize {
    RenderSize {
        width: width.min(u16::MAX as u32) as u16,
        height: height.min(u16::MAX as u32) as u16,
    }
}
