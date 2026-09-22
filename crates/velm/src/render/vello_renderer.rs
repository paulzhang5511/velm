//! render/vello_renderer.rs — vello 0.10 / wgpu 29 渲染器（SPEC §7.8）。
//!
//! 链路：`NativeWindowWrapper`（rwh 0.6 句柄）→ wgpu 29 unsafe surface
//! （Vulkan）→ adapter/device → vello Scene 编码到 Rgba8Unorm 中间纹理
//! （compute）→ `TextureBlitter` 上屏 → present。
//!
//! 本模块依赖 wgpu/vello/skrifa（android-only），**只在 android 目标编译**；
//! 「画什么」由 `render::scene`（host 可测）决定，本模块只负责「怎么画」。

use std::num::NonZeroUsize;

use raw_window_handle::HasDisplayHandle;
use raw_window_handle::HasWindowHandle;
use vello::kurbo::{Affine, Rect, RoundedRect};
use vello::peniko::Color;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use wgpu;

use crate::error::{Error, Result};
use crate::platform::window::NativeWindowWrapper;
use crate::render::font::FontCache;
use crate::render::scene::{DrawCommand, centered_baseline};
use crate::view::View;

/// ADR-10 背景色 #121212（peniko from_rgba8 内部转线性，非 sRGB surface 直通）。
const BACKGROUND: Color = Color::from_rgba8(0x12, 0x12, 0x12, 0xFF);

/// vello 渲染器：持有 wgpu 表面/设备、vello 渲染器、中间纹理与字体缓存。
pub struct VelloRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    vello: Renderer,
    blitter: wgpu::util::TextureBlitter,
    /// vello compute 写入的中间纹理（Rgba8Unorm + STORAGE）。
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
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
                // vello compute 管线需要完整 default limits（SwiftShader Vulkan1.3 支持）。
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
        // vello 的 fine pass 输出已是 sRGB 编码字节值（约定 target 为线性
        // Rgba8Unorm，内部完成线性混合与 sRGB 编码）。surface 必须用**非 sRGB**
        // Rgba8Unorm，blit 时值直通；若用 Rgba8UnormSrgb 会被硬件二次编码
        // 导致整体泛白（#121212→#4a4a4a）。Rgba8Unorm 在 Vulkan 上保证支持。
        config.format = wgpu::TextureFormat::Rgba8Unorm;
        // Fifo 是唯一保证支持的呈现模式（ADR-09）。
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        // vello 仅启用 Area AA（官方推荐默认，shader 变体最少、启动最快）；
        // 单线程初始化 shader，避免软件 Vulkan 下并行编译的内存/竞态风险。
        let vello = Renderer::new(
            &device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                ..Default::default()
            },
        )
        .map_err(|e| Error::VelloInit(e.to_string()))?;
        // blitter 目标格式 = surface 格式（Rgba8Unorm，非 sRGB），源格式不限。
        let blitter = wgpu::util::TextureBlitter::new(&device, config.format);
        let (target, target_view) = create_vello_target(&device, width, height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            vello,
            blitter,
            target,
            target_view,
            fonts: FontCache::load(),
            density,
            last_frame: Vec::new(),
        })
    }

    /// 当前 surface 尺寸（物理像素）。
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// 窗口尺寸变化：重配 swapchain 与 vello 中间纹理，并用上一帧的指令重画
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
        let (target, view) = create_vello_target(&self.device, width, height);
        self.target = target;
        self.target_view = view;
        // 借用 last_frame 后再交给不需要 &mut self 的编码路径，故先取出。
        let commands = std::mem::take(&mut self.last_frame);
        if !commands.is_empty() {
            self.present(&commands);
            self.last_frame = commands;
        }
    }

    /// 渲染一帧：视图树 → 绘制指令 → vello 编码 → present。
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

        let scene = self.encode(commands);
        let (width, height) = (self.config.width, self.config.height);

        // vello compute 写入 Rgba8Unorm 中间纹理（base_color 负责清屏）。
        if let Err(e) = self.vello.render_to_texture(
            &self.device,
            &self.queue,
            &scene,
            &self.target_view,
            &RenderParams {
                base_color: BACKGROUND,
                width,
                height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            log::warn!("vello render_to_texture 失败: {e}");
            return;
        }

        // blit 中间纹理到非 sRGB surface（值直通，vello 已完成 sRGB 编码）。
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("velm-blit"),
            });
        self.blitter
            .copy(&self.device, &mut encoder, &self.target_view, &frame_view);
        self.queue.submit([encoder.finish()]);
        frame.present();
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

    /// 把绘制指令编码进 vello `Scene`（顺序即画家算法）。
    fn encode(&self, commands: &[DrawCommand]) -> Scene {
        let mut scene = Scene::new();
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
                    let rounded = RoundedRect::from_rect(rect, *corner_radius as f64);
                    scene.fill(
                        vello::peniko::Fill::NonZero,
                        Affine::IDENTITY,
                        *color,
                        None,
                        &rounded,
                    );
                }
                DrawCommand::Text {
                    rect,
                    text,
                    size_px,
                    color,
                } => {
                    let (ascent, descent) = self.fonts.vertical_metrics(*size_px);
                    let baseline = rect.y + centered_baseline(rect.height, ascent, descent);
                    for run in self.fonts.shape_line(text, *size_px) {
                        scene
                            .draw_glyphs(&run.face.data)
                            .font_size(*size_px)
                            .brush(*color)
                            .transform(Affine::translate((rect.x as f64, baseline as f64)))
                            .draw(vello::peniko::Fill::NonZero, run.glyphs.into_iter());
                    }
                }
            }
        }
        scene
    }
}

/// 创建 vello 渲染目标：Rgba8Unorm（vello 要求），STORAGE（compute 写）+
/// TEXTURE_BINDING（blitter 采样）。
fn create_vello_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("velm-vello-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
