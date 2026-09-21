//! T2 spike：wgpu surface 清屏 POC（T10 演进为 `VelloRenderer`，SPEC §7.8）。
//!
//! 本模块验证最高风险链路：`ANativeWindow` 裸指针 → raw-window-handle 0.6
//! 句柄 → wgpu 29 unsafe surface（Vulkan）→ adapter/device → present。
//! vello Scene 在 Slice 2 接入；当前仅清屏 #121212 一帧（同时满足
//! 「首帧建立输入窗口几何」约束，替代 T3 的软件 probe frame）。

use std::ffi::c_void;
use std::ptr::NonNull;

use raw_ndk_sys::ANativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wgpu;

/// ADR-10 背景色 #121212。surface 取 caps 首选的 `Rgba8UnormSrgb`（硬件做
/// sRGB 编码），故清屏色须给线性值：18/255 sRGB ≈ 0.00605 linear。
/// Slice 2 接入 vello 后由 peniko `Color::from_rgba8` 统一处理。
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.00605,
    g: 0.00605,
    b: 0.00605,
    a: 1.0,
};

/// 最小 wgpu 表面封装：创建后清屏一帧，窗口尺寸变化时重配。
pub struct WgpuClear {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl WgpuClear {
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
        // 模拟器 ranchu/ranchu→SwiftShader Vulkan 虽声明 VK_EXT_debug_utils，
        // 其 vkSetDebugUtilsObjectNameEXT 实现有缺陷（设备创建期设置资源名即
        // 段错误，见 T2 spike 记录）。丢弃 HAL 层 debug 名可绕过，不影响渲染；
        // 真机 Mali/Adreno 无此问题，统一关闭也无功能损失。
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
                // 移动 Vulkan/模拟器广泛支持的 WebGL2 级下限；vello 接入后按其要求上调。
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
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
        // Fifo 是唯一保证支持的呈现模式（ADR-09）。
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        Some(Self {
            surface,
            device,
            queue,
            config,
        })
    }

    /// 窗口尺寸变化时重新配置 swapchain 并重绘一帧。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 && (width != self.config.width || height != self.config.height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.render_clear();
        }
    }

    /// 清屏一帧并 present；任何环节失败只告警不 panic（ADR-11）。
    pub fn render_clear(&self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            other => {
                log::warn!("获取交换链图像失败，跳过本帧: {other:?}");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("velm-clear"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("velm-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(BACKGROUND),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
