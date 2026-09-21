# T2 渲染能力 spike：vello 0.10 / wgpu 29 / raw-window-handle 0.6 / 文本 POC

**日期**：2026-09-22
**对应任务**：PLAN Task 2（渲染 spike）、ADR-03（渲染栈版本）、ADR-06（文本栈，Slice 3 决策）
**设备**：x86_64 / API 36 模拟器（AVD velm_test，swiftshader_indirect，320×640，density 160）
**状态**：Slice 1 ✅ ｜ Slice 2 ⬜ vello Scene ｜ Slice 3 ⬜ 文本 ｜ Slice 4 ⬜ resize

本 spike 以消除渲染路径不确定性为目标，代码直接落在框架 `crates/velm/src/render/`
（T10 `VelloRenderer` 的前身），允许原型质量，但每个关键 API 与设备行为必须真机实证。

---

## 1. 切片计划（risk-first）

| Slice | 目标 | 最高风险 | 状态 |
|---|---|---|---|
| 1 | `ANativeWindow` → rwh 0.6 → wgpu 29 surface（Vulkan）→ adapter/device → 清屏 present | 模拟器软件 Vulkan 是否可用、wgpu 29 API 变迁 | ✅ 2026-09-22 |
| 2 | 引入 vello 0.10（`features=["wgpu"]`）+ peniko 0.6，Scene 清屏 + 圆角矩形 | vello 0.10 真实 API 序列、limits 要求 | ⬜ |
| 3 | glifo 0.2 vs skrifa 0.44 时间盒；Roboto + NotoSansCJK 中英文 | 文本栈选型、ttc index、AssetManager 读字体 | ⬜ |
| 4 | `onNativeWindowResized` → surface 重配；旋转不崩；ADR-06 定稿 | resize 时序、旧帧失效处理 | ⬜ |

---

## 2. Slice 1：wgpu surface 清屏链路（✅ 真机通过）

### 2.1 依赖（`crates/velm/Cargo.toml`，android target）

```toml
wgpu = { version = "29", default-features = false, features = ["vulkan"] }  # 解析为 29.0.4
pollster = "0.4"                                                              # 0.4.0
raw-window-handle = "0.6"                                                     # 0.6.2
```

vello / peniko 在 Slice 2 引入。wgpu 关闭默认 feature、仅开 `vulkan`（Android 唯一后端；
ash 在运行时 `dlopen libvulkan.so`，无需额外链接参数）。

### 2.2 已验证的 wgpu 29.0.4 真实 API 序列

> 29 与网上常见的 22/23 示例差异很大，以下全部经编译 + 真机确认。

1. **Instance**：`InstanceDescriptor` 不再实现 `Default`，用
   `wgpu::InstanceDescriptor::new_without_display_handle()` 构造（按值传给
   `Instance::new(desc)`，不是引用），再设 `backends = Backends::VULKAN`。
2. **Surface（unsafe）**：
   ```
   AndroidNdkWindowHandle::new(NonNull::new(window as *mut c_void)?)
   RawWindowHandle::AndroidNdk(..)
   AndroidDisplayHandle::new() → RawDisplayHandle::Android(..)
   instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
       raw_display_handle: Some(RawDisplayHandle),
       raw_window_handle: RawWindowHandle,
   })?  // create_surface_unsafe 是 unsafe fn
   ```
   surface 标注 `Surface<'static>`：引擎线程持有 acquire 过的 window，渲染器先 drop、
   窗口后 release，满足句柄生命周期契约。
3. **Adapter**：`request_adapter(&RequestAdapterOptions { power_preference,
   force_fallback_adapter, compatible_surface: Some(&surface) })`（`wgpu::RequestAdapterOptions`
   是 `RequestAdapterOptionsBase<&Surface>` 的别名）。
4. **Device**：`request_device(&DeviceDescriptor { label, required_features, required_limits,
   experimental_features, memory_hints, trace })` —— **只接收 1 个参数**（旧版第二个
   trace path 参数已移除）；`experimental_features` 用 `ExperimentalFeatures::disabled()`
   （无 `::empty()`）；29 用 `memory_hints: MemoryHints`。
5. **Limits**：`Limits::downlevel_webgl2_defaults()` 被 SwiftShader 接受（default limits
   未测；Slice 2 按 vello 要求评估是否上调）。
6. **Surface 配置**：`surface.get_default_config(&adapter, w, h)` 返回
   `Option<wgpu::SurfaceConfiguration>`（类型别名已固定 `view_formats: Vec<TextureFormat>`，
   不再需要手写泛型）；随后强制 `present_mode = PresentMode::Fifo`（唯一保证支持）。
7. **取帧**：29 的 `get_current_texture()` **直接返回枚举 `CurrentSurfaceTexture`**，
   不再返回 `Result`：`Success(SurfaceTexture)` / `Suboptimal(SurfaceTexture)` 可用，
   `Timeout / Occluded / Outdated / Lost / Validation` 应跳过本帧（Outdated/Lost 需
   reconfigure/重建，Slice 4 resize 处理）。
8. **清屏 present**：`create_view` → `create_command_encoder` →
   `begin_render_pass(RenderPassDescriptor { .., depth_slice: None（ColorAttachment 新增）,
   multiview_mask: None（Descriptor 新增）, timestamp_writes/occlusion_query_set: None })`
   → `queue.submit([enc.finish()])` → `frame.present()` →
   `device.poll(wgpu::PollType::wait_indefinitely())`（29 的 poll 取 `PollType`，
   非旧 `Maintain`；返回 `Result<PollStatus, PollError>`）。

### 2.3 真机实测能力（SwiftShader / Vulkan）

```
adapter = "SwiftShader Device (Subzero)"  vendor=6880 device=49374 type=Cpu backend=Vulkan
surface formats = [Rgba8UnormSrgb, Rgba8Unorm, Rgba16Float, Rgb10a2Unorm]
present_modes   = [Mailbox, Fifo]
alpha_modes     = [Inherit]
```

- 选 `get_default_config` 的首选格式 = **Rgba8UnormSrgb**（硬件做 sRGB 编码）。
- **颜色空间坑**：sRGB surface 下 `LoadOp::Clear(Color)` 的分量是**线性值**。
  ADR-10 背景 `#121212`（sRGB 18/255=0.0706）直接填会显示成偏亮灰（约 #303030）；
  线性化后 ≈ **0.00605** 才正确显示 #121212。Slice 2 起由 peniko
  `Color::from_rgba8` 统一处理，清屏常量随之迁移。
- 截图：全屏近黑 #121212（`/tmp/velm_wgpu_121212.png`，临时非交付物）。

### 2.4 关键坑：模拟器 ranchu Vulkan 的 debug_utils 段错误（必须记录）

首次装机在 `request_device` 内部**原生崩溃**（SIGSEGV），栈顶：

```
#00 vulkan.ranchu.so  vk_common_SetDebugUtilsObjectNameEXT
#01 ash              Device::set_debug_utils_object_name
#02 wgpu_hal::vulkan  DeviceShared::set_object_name
#03 wgpu_hal::vulkan  create_bind_group_layout
...
#07 wgpu_core        Device::new
```

模拟器 GPU 转发层 `vulkan.ranchu.so`（swiftshader_indirect 模式下 ranchu→SwiftShader）
**声明了 `VK_EXT_debug_utils`，但 `vkSetDebugUtilsObjectNameEXT` 实现有缺陷**，wgpu
在设备创建期为内部资源设置 debug 名时即段错误（与我们是否传 label 无关，内部 bind
group layout 也有名）。

**解法**：InstanceFlags 置 `DISCARD_HAL_LABELS`（丢弃 HAL 层 debug 名，不向下调用
`vkSetDebugUtilsObjectNameEXT`），不影响任何渲染功能；真机 Mali/Adreno 无此缺陷，
统一开启也无损失。已在代码中注释记录。

### 2.5 生命周期与压测

- 渲染器存于引擎线程局部 `Option<WgpuClear>`：`WindowCreated` → `new()` + 一帧清屏；
  `WindowDestroyed` / `Quit` → **先 `take()` drop 渲染器（销毁 surface/device/queue），
  再 `ANativeWindow_release`**，顺序不可反（surface 持有 window 句柄）。
- wgpu GPU 首帧与 T3 的软件 probe frame 等效建立输入窗口几何：清屏帧 present 后
  `input tap 160 320` 的 DOWN/UP 立即到达（软件 `post_probe_frame` 已删除，
  同一 ANativeWindow 不允许软件 lock 与 wgpu 并用）。
- **压测**：force-stop 后连续 5 次 start→tap→BACK：**5/5 wgpu 初始化成功、5/5 干净
  join、5/5 触摸到达、0 原生崩溃、0 wgpu 校验错误**。BACK 后进程作为空缓存进程留存
  （Android 正常行为），`onDestroy`/引擎线程 join/ Vulkan 资源销毁均已完成。

### 2.6 对 T10（正式渲染器）的约束

1. Instance 固定 `Backends::VULKAN` + `InstanceFlags::DISCARD_HAL_LABELS`。
2. surface 用 `create_surface_unsafe` + `Surface<'static>`，句柄 unsafe 构造是 render
   模块内唯一的 unsafe 边界（SPEC 要求 render 编码逻辑 safe；句柄构造 T10 评估是否
   收敛到 platform 模块）。
3. 呈现模式固定 Fifo；格式取 caps 首选 sRGB（vello 接入后复核），颜色一律走线性空间
   或 peniko 转换，禁止直接填 sRGB 分量。
4. `get_current_texture` 按枚举处理：Outdated/Lost 重配/重建，Timeout/Occluded 跳帧，
   任何失败 log + 恢复，**不 panic**（ADR-11）。
5. 销毁顺序：drop renderer → release window；与 T3 同步 ack 协议兼容。
6. limits 先用 downlevel_webgl2；Slice 2 若 vello 需要更高 limits，在真机/模拟器分别
   核对支持情况再上调。
