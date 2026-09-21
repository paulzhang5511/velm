//! T2 spike：vello 0.10 / wgpu 29 渲染 POC（T10 演进为正式 `VelloRenderer`，SPEC §7.8）。
//!
//! 链路：`ANativeWindow` 裸指针 → raw-window-handle 0.6 → wgpu 29 unsafe
//! surface（Vulkan）→ adapter/device → vello Scene 编码到 Rgba8Unorm 中间
//! 纹理（compute）→ `TextureBlitter` 上屏（surface 为 sRGB 格式，blit 时
//! 硬件做 sRGB 编码）→ present。
//!
//! Slice 2：清屏 #121212（vello `RenderParams::base_color`）+ 一个绿色
//! 圆角矩形；文本在 Slice 3 接入。

use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::ptr::NonNull;

use raw_ndk_sys::ANativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use vello::kurbo::{Affine, Rect, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use wgpu;

/// ADR-10 背景色 #121212（peniko from_rgba8 内部转线性，sRGB surface 正确）。
const BACKGROUND: Color = Color::from_rgba8(0x12, 0x12, 0x12, 0xFF);
/// ADR-10 「+1」绿。
const ACCENT: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);

/// vello 渲染器 POC：持有 wgpu 表面、vello Renderer 与中间纹理。
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
}

impl VelloRenderer {
    /// 在引擎线程调用。
    ///
    /// # Safety
    /// `window` 必须是已 `ANativeWindow_acquire` 的有效窗口，且其所有权保证
    /// 本结构 drop 之前窗口不被 release（销毁协议：先 drop 渲染器再 release）。
    pub fn new(window: *mut ANativeWindow, width: u32, height: u32) -> Option<Self> {
        if window.is_null() || width == 0 || height == 0 {
            return None;
        }
        pollster::block_on(Self::init(window, width, height))
    }

    async fn init(window: *mut ANativeWindow, width: u32, height: u32) -> Option<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        // 模拟器 ranchu→SwiftShader 虽声明 VK_EXT_debug_utils，其
        // vkSetDebugUtilsObjectNameEXT 实现有缺陷（设备创建期段错误）；
        // 丢弃 HAL 层 debug 名绕过，真机无此问题，统一关闭无功能损失。
        descriptor.flags = wgpu::InstanceFlags::DISCARD_HAL_LABELS;
        let instance = wgpu::Instance::new(descriptor);

        // SAFETY: 调用方保证 window 在 surface 存活期间有效（见 new 的 Safety）。
        let surface = unsafe {
            let ptr = NonNull::new(window as *mut c_void)?;
            let raw_window_handle = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(ptr));
            let raw_display_handle = RawDisplayHandle::Android(AndroidDisplayHandle::new());
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .ok()?
        };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .ok()?;

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
            .ok()?;

        let caps = surface.get_capabilities(&adapter);
        log::info!(
            "wgpu adapter={:?} formats={:?} present_modes={:?} alpha={:?}",
            adapter.get_info().name,
            caps.formats,
            caps.present_modes,
            caps.alpha_modes
        );

        let mut config = surface.get_default_config(&adapter, width, height)?;
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
        .ok()?;
        // blitter 目标格式 = surface 格式（sRGB），源（vello 中间纹理）格式不限。
        let blitter = wgpu::util::TextureBlitter::new(&device, config.format);
        let (target, target_view) = create_vello_target(&device, width, height);

        Some(Self {
            surface,
            device,
            queue,
            config,
            vello,
            blitter,
            target,
            target_view,
        })
    }

    /// 窗口尺寸变化：重配 swapchain 与 vello 中间纹理并重绘一帧。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 && (width != self.config.width || height != self.config.height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            let (target, view) = create_vello_target(&self.device, width, height);
            self.target = target;
            self.target_view = view;
            self.render_frame();
        }
    }

    /// 编码一帧（清屏 + 圆角矩形）并 present；失败只告警不 panic（ADR-11）。
    pub fn render_frame(&mut self) {
        let (width, height) = (self.config.width, self.config.height);

        let mut scene = Scene::new();
        // 居中的绿色圆角矩形（POC 几何，证明 vello 形状光栅化链路）。
        let rect = Rect::new(40.0, 250.0, 280.0, 390.0);
        let rounded = RoundedRect::from_rect(rect, 20.0);
        scene.fill(Fill::NonZero, Affine::IDENTITY, ACCENT, None, &rounded);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            other => {
                log::warn!("获取交换链图像失败，跳过本帧: {other:?}");
                return;
            }
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

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

        // blit 中间纹理到 sRGB surface（硬件完成 sRGB 编码）。
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
