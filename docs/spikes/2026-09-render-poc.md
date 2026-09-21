# T2 渲染能力 spike：vello 0.10 / wgpu 29 / raw-window-handle 0.6 / 文本 POC

**日期**：2026-09-22
**对应任务**：PLAN Task 2（渲染 spike）、ADR-03（渲染栈版本）、ADR-06（文本栈，Slice 3 决策）
**设备**：x86_64 / API 36 模拟器（AVD velm_test，swiftshader_indirect，320×640，density 160）
**状态**：Slice 1 ✅ ｜ Slice 2 ✅ vello Scene ｜ Slice 3 ✅ 文本（skrifa 直绘）｜ Slice 4 ⬜ resize

本 spike 以消除渲染路径不确定性为目标，代码直接落在框架 `crates/velm/src/render/`
（T10 `VelloRenderer` 的前身），允许原型质量，但每个关键 API 与设备行为必须真机实证。

---

## 1. 切片计划（risk-first）

| Slice | 目标 | 最高风险 | 状态 |
|---|---|---|---|
| 1 | `ANativeWindow` → rwh 0.6 → wgpu 29 surface（Vulkan）→ adapter/device → 清屏 present | 模拟器软件 Vulkan 是否可用、wgpu 29 API 变迁 | ✅ 2026-09-22 |
| 2 | 引入 vello 0.10（`features=["wgpu"]`）+ peniko 0.6，Scene 清屏 + 圆角矩形 | vello 0.10 真实 API 序列、limits 要求 | ✅ 2026-09-22 |
| 3 | glifo 0.2 vs skrifa 0.44 时间盒；Roboto + NotoSansCJK 中英文 | 文本栈选型、ttc index、AssetManager 读字体 | ✅ 2026-09-22（定稿 skrifa，ADR-06） |
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
   核对支持情况再上调。**（Slice 2 已复核：vello 需要 `Limits::default()`，surface 格式
   改为非 sRGB，见 §3.6，以 §3 为准。）**

---

## 3. Slice 2：vello Scene 圆角矩形（✅ 真机通过）

在 Slice 1 的 wgpu surface 链路上接入 vello，把 `WgpuClear` 演进为 `VelloRenderer`，
用 vello Scene 绘制 ADR-10 的绿色圆角矩形，证明 **vello compute 光栅化 → 中间纹理 →
blit 上屏** 的完整 2D 渲染路径。

### 3.1 依赖（`crates/velm/Cargo.toml`，android target）

```toml
vello  = { version = "0.10", default-features = false, features = ["wgpu"] }
peniko = "0.6"
```

`cargo tree` 实测解析为单一 wgpu 版本，无重复：

```
vello 0.10.0
├── peniko 0.6.1
├── kurbo 0.13.1
├── skrifa 0.44.0        # vello 内部已带（Slice 3 / ADR-06 的关键事实）
└── wgpu 29.0.4          # 与我们直接依赖的 wgpu 29 对齐，全树唯一版本
```

vello 关闭默认 feature、仅开 `wgpu`（不引入其平台样板）；peniko/kurbo/skrifa 均通过
`vello::peniko` / `vello::kurbo` re-export 使用，无需直接声明 kurbo/skrifa。

### 3.2 已验证的 vello 0.10 真实 API 序列

> 0.10 与网上 0.7/0.8 示例差异较大（无 `render_to_surface`），以下经编译 + 真机确认。

1. **Renderer**：`vello::Renderer::new(&device, RendererOptions) -> Result<Renderer>`
   （不是旧版 `new_device`）。选项：
   ```
   RendererOptions {
       use_cpu: false,                                  // 默认
       antialiasing_support: AaSupport::area_only(),    // 仅 Area AA，shader 变体最少
       num_init_threads: NonZeroUsize::new(1),          // 单线程编译 shader
       pipeline_cache: None,
   }
   ```
   `Renderer::new` 会在此时编译/初始化全部 shader pipeline；SwiftShader 上单线程
   area_only 约亚秒~秒级，发生在引擎线程（不阻塞主线程）。
2. **中间纹理（vello 渲染目标）**：0.10 **没有 `render_to_surface`**，官方推荐渲染到
   一张中间纹理再 blit。目标纹理必须：
   - `format = Rgba8Unorm`（**非 sRGB**，vello 硬性要求）；
   - `usage = STORAGE_BINDING`（compute 写入）**| `TEXTURE_BINDING`**（blitter 采样）。
3. **Scene 编码**：
   ```
   let mut scene = vello::Scene::new();
   let rect = kurbo::Rect::new(40.0, 250.0, 280.0, 390.0);
   let rounded = kurbo::RoundedRect::from_rect(rect, 20.0);   // 半径 20px
   scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY,
              peniko::Color::from_rgba8(0x2E,0x7D,0x32,0xFF), None, &rounded);
   ```
   `Scene::fill(style, transform, brush: impl Into<BrushRef>, brush_transform, shape: &impl Shape)`。
   **没有 `Scene::clear`**：清屏由 `RenderParams::base_color` 负责。
4. **渲染到纹理**：
   ```
   vello.render_to_texture(&device, &queue, &scene, &target_view, &RenderParams {
       base_color: Color::from_rgba8(0x12,0x12,0x12,0xFF), // 背景清屏
       width, height,
       antialiasing_method: AaConfig::Area,
   })?;   // 返回 Result<()>，内部自行 submit compute pass
   ```
5. **blit 上屏**：`wgpu::util::TextureBlitter::new(&device, target_format)`，每帧
   `blitter.copy(&device, &mut encoder, &source_view, &target_surface_view)`（内部是一个
   `LoadOp::Load` 的全屏三角形 render pass；**source 格式不限、target 格式必须等于
   `new` 时传入的格式**）。随后 `queue.submit([encoder.finish()])` → `frame.present()` →
   `device.poll(wait_indefinitely())`。
6. **取帧/呈现/poll** 与 Slice 1 完全相同（`CurrentSurfaceTexture` 枚举、Fifo）。

### 3.3 关键坑：色彩管线双重 sRGB 编码（最重要结论）

首次按 Slice 1 的做法让 surface 取 caps 首选 **Rgba8UnormSrgb**，画面整体**泛白**：
背景 #121212 显示成约 #4a4a4a，深绿 #2E7D32 显示成浅绿（截图对照
`/tmp/velm_vello_rect.png`）。

根因：**vello fine pass 输出到 Rgba8Unorm 纹理的字节值已经是 sRGB 编码结果**
（vello 内部在线性空间混合，输出阶段完成线性→sRGB 编码，约定目标为「线性标签」的
Rgba8Unorm）。再把它 blit 到 `Rgba8UnormSrgb` surface，硬件在 render pass 输出时
**又做一次线性→sRGB 编码**，于是二次编码、整体提亮。

**解法（权威结论）**：全链路统一非 sRGB、值直通——

- surface `config.format = Rgba8Unorm`（不用 caps 首选的 sRGB；Rgba8Unorm 在 Vulkan
  上保证支持，实测 caps 列表中含）；
- TextureBlitter target format 同为 Rgba8Unorm；
- vello 中间纹理本就是 Rgba8Unorm。

三处格式一致后 blit 不做任何 gamma 转换，vello 输出什么字节就显示什么。修正后截图
颜色完全正确：近黑 #121212 底 + #2E7D32 深绿圆角矩形、圆角抗锯齿清晰
（`/tmp/velm_vello_color.png`，临时非交付物）。

> 对比 Slice 1：纯 wgpu `LoadOp::Clear` 直接清 sRGB surface 时，clear 值被当作**线性**
> 值由硬件编码一次，所以那时要填线性 0.00605。接入 vello 后编码责任转移给 vello，
> surface 必须退回非 sRGB。两条路径的颜色空间约定不同，T10 统一走 vello 路径，
> 一律以本条为准。

### 3.4 limits 上调为 default

Slice 1 的 `Limits::downlevel_webgl2_defaults()` 不足以支撑 vello 的 compute 管线
（大量 storage texture/buffer 与 workgroup 需求）。改为 **`wgpu::Limits::default()`**，
SwiftShader Vulkan 1.3 完整接受、`request_device` 成功、渲染无校验错误。真机
Mali/Adreno 的 default limits 支持情况留待 arm64 真机复核（当前仅 x86_64 模拟器动态验证
+ aarch64 静态编译/clippy 通过）。

### 3.5 生命周期、销毁与压测

- 渲染器字段（按 drop 顺序）：vello `Renderer`、`TextureBlitter`、中间 `Texture`/`TextureView`、
  surface/device/queue。`WindowDestroyed`/`Quit` 仍遵循「先 `take()` drop 渲染器、再
  `ANativeWindow_release`」。实测 BACK 时 vello 全部 GPU 资源释放**无错误、无校验告警、
  无原生崩溃**。
- **压测**：force-stop 后 3 次 start（同步等待每轮 adapter 日志出现）→tap→BACK：
  **3/3 vello 初始化成功、3/3 干净 join、3/3 触摸到达、`render_to_texture` 失败 0、
  原生崩溃 0**。
- **日志 ring buffer 坑（排查记录）**：早期固定时长压测出现「adapter 计数 2/3、join
  3/3」的假象。根因不是框架，而是 `android_logger` 开在 **Trace** 级，vello 每次
  `Renderer::new` 触发 naga/wgpu 海量 Trace 日志，**冲爆 logcat ring buffer**，把较早的
  adapter 行挤出（join 行较新故保留）。已把日志级别降为 **Info**（框架自身只用
  info/warn/error），降级后同样 3 轮压测 adapter=3、join=3、crash=0，计数稳定。
  副作用：同时显著减少软件 Vulkan 启动期的日志 I/O。
- 触摸在 vello 首帧 present 后正常到达（GPU 首帧同样建立输入窗口几何）。

### 3.6 对 T10（正式渲染器）的约束（更新并覆盖 §2.6 相关条目）

1. surface 格式固定 **Rgba8Unorm（非 sRGB）**；vello 中间纹理 Rgba8Unorm +
   `STORAGE_BINDING | TEXTURE_BINDING`；TextureBlitter target 与 surface 同格式。
   颜色一律用 peniko `Color::from_rgba8`，由 vello 负责 sRGB 编码，**不要再填线性
   clear 值、不要用 sRGB surface**。
2. device limits 用 **`wgpu::Limits::default()`**（vello 要求），真机复核。
3. vello `Renderer::new` 用 `AaSupport::area_only()` + `num_init_threads = 1`；
   AA 方法运行期传 `AaConfig::Area`。
4. 每帧：Scene 编码（清屏走 `RenderParams::base_color`）→ `render_to_texture` →
   TextureBlitter `copy` → submit → present → poll；`render_to_texture` 返回的错误
   log 并跳过本帧，不 panic。
5. resize（Slice 4）需同时重配 surface **并重建中间纹理**（`create_vello_target`
   已封装），再补一帧。
6. 日志级别固定 Info，禁止在装机验证构建开 Trace（ring buffer 会丢早期关键日志）。
7. 其余 Instance/surface unsafe/Fifo/取帧枚举/销毁顺序沿用 §2.6。

---

## 4. Slice 3：文本栈（skrifa 直绘）与中英文渲染（✅ 真机通过，ADR-06 定稿）

### 4.1 选型结论：skrifa 0.44 直绘，不引入 glifo

vello 0.10 只提供 glyph run 编码（`Scene::draw_glyphs`），**不含字体解析、cmap、排版**。
时间盒对比两条路径（完整论证见 ADR-06）：

| 维度 | skrifa 0.44 直绘（**选定**） | glifo 0.2 |
|---|---|---|
| 与 vello 0.10 关系 | vello 内部已依赖 skrifa 0.44，`draw_glyphs` 内部也用它解析 | 依赖 `vello_common 0.1`（下一代 vello_hybrid 栈），renderer 仅 `use vello_common::paint`，**不对接 vello 0.10 Scene** |
| 新增依赖 | **无解析栈新增**（显式声明同版本 skrifa） | +hashbrown/foldhash/smallvec/bytemuck/vello_common（可选 png） |
| 能力 | cmap + 水平 advance（够用） | glyph atlas 缓存、下划线/删除线、富文本（P0 用不到） |
| 成熟度 | 随 vello 0.10 稳定 | self-described experimental，0.2→0.3 快速迭代 |

P0 仅单行数字 + 中英文标签（无 emoji/复杂连字/BiDi/自动换行），skrifa 直绘最小且与
ADR-03 同栈；glifo 留待升级 vello_common 架构或需要富文本时再评估。

新增依赖（android target）：`skrifa = "0.44"`（解析为 0.44.0，与 vello 内部完全同版本）。

### 4.2 系统字体与 ttc index（真机实测）

字体**不打包进 APK**，直接 `std::fs::read` 系统路径（对所有进程可读；NotoSansCJK ttc
约 32MB，打包会让 APK 严重膨胀）：

```
/system/fonts/Roboto-Regular.ttf        2,371,712 B   拉丁/数字（ttf，index 0）
/system/fonts/NotoSansCJK-Regular.ttc  32,355,424 B   CJK（ttc，含 5 个子表）
```

host 端用 skrifa 枚举 ttc（临时探针，非交付物）实测子表：

```
index 0: Noto Sans CJK JP   （含 CJK 统一表意文字，但日式字形）
index 1: Noto Sans CJK KR
index 2: Noto Sans CJK SC   ← 简体中文，选这个
index 3: Noto Sans CJK TC
index 4: Noto Sans CJK HK
```

五个子表对同一 CJK 码位都有 glyph（Unicode 统一表意文字共享），差别是**地区字形**；
简体必须用 **index 2 (SC)**。Roboto 实测 `'0'→gid21`、`'A'→gid38`、`'计'→无`（中文
正确回退 Noto）。Noto SC 全角汉字 28px 下 advance≈28（1em）。

### 4.3 已验证的 skrifa 0.44 / vello glyph API

- 字体引用：`skrifa::FontRef::from_index(&bytes, ttc_index)?`（ttf 用 `FontRef::new`），
  字节需在引用期间存活。
- cmap：`use skrifa::MetadataProvider; let charmap = font.charmap(); charmap.map(char) -> Option<GlyphId>`；
  `GlyphId::to_u32()`。
- 水平度量：`font.glyph_metrics(Size::new(px), LocationRef::default()).advance_width(gid) -> Option<f32>`
  （`Size`/`LocationRef` 在 `skrifa::instance`，prelude 亦导出）。
- family name（诊断）：`font.localized_strings(StringId::FAMILY_NAME).english_or_first()`。
- vello 字体数据：`vello::peniko::{Blob, FontData}`（peniko re-export 自
  linebender_resource_handle）：`FontData::new(Blob::new(Arc<Vec<u8>>>), index)`；
  `Blob::new(Arc<dyn AsRef<[u8]> + Send + Sync>)`，与自持有字节共享同一分配。
- glyph run：`vello::Glyph { id: u32, x: f32, y: f32 }`（x/y 相对 run 原点，y=0 即基线）；
  ```
  scene.draw_glyphs(&font_data)
       .font_size(px)
       .brush(color)
       .transform(Affine::translate((origin_x, baseline_y)))  // run 原点=基线左端
       .draw(Fill::NonZero, glyphs.into_iter());               // DrawGlyphs builder
  ```
  `draw_glyphs` 是 `Scene` 固有方法（无需 trait import）；清屏仍走 `RenderParams::base_color`。

### 4.4 实现结构（`crates/velm/src/render/text.rs`）

- `FontFace { bytes: Arc<Vec<u8>>, index, data: FontData, family }`：`load(path,index)`
  从系统路径加载；`shape(text,px) -> (Vec<Glyph>, 行宽)` 逐字符 cmap + advance；
  `contains(char)` 判字体覆盖。
- `shape_runs(text, px, primary: &FontFace, fallback: Option<&FontFace>) -> Vec<GlyphRun>`：
  逐字符选字体（primary 含则用 primary，否则 fallback，都不含跳过），连续同字体的字符
  聚成一段，段内 glyph x 累加为整行偏移；绘制时各 run 共用同一行原点 transform。
- 渲染器持有 `roboto: Option<FontFace>`、`noto_sc: Option<FontFace>`，在 `init` 末尾
  `std::fs::read` 加载（失败只 warn，不阻断图形）。

### 4.5 真机结果与压测

- 画面（`/tmp/velm_text_cjk.png`，临时非交付物）：白字 28px 两行——
  行1 Roboto「Count: 0」；行2「计数 +1」中「计数」为 Noto Sans CJK **简体字形**、
  空格与「+1」回退 Roboto，基线对齐、字号一致；绿色圆角矩形与 #121212 背景不变。
- 加载日志：`已加载字体 Roboto（…#0，2371712 字节）`、
  `已加载字体 Noto Sans CJK SC（…#2，32355424 字节）`。
- **压测**：force-stop 后 3 次 start（同步等待 Noto 加载日志）→tap→BACK：
  **3/3 两字体加载、3/3 干净 join、渲染/加载失败 0、原生崩溃 0**。32MB ttc 读取与
  skrifa 解析在引擎线程完成，未阻塞主线程、未显著拖慢启动。

### 4.6 对 T10/T6 的约束

1. 文本统一走 skrifa：`FontFace::shape`/`shape_runs` 是排版层（T6）与渲染层（T10）的
   边界；T10 只消费 `(FontData, Vec<Glyph>, 字号, 颜色, 原点)`。
2. WrapContent 文本宽高用 skrifa 真实度量（advance + ascent/descent），废弃
   SPEC §7.4 的 `chars*size*0.6` 近似；中文按全角（advance≈1em）。
3. 字体路径/ttc index 作为常量；T15 评估在缺少某字体的设备上的 fallback 策略
   （API24 AOSP/主流机型均含 Roboto + NotoSansCJK）。
4. 不做 kerning/复杂 shaping/换行（P1）；若后续需要，重新评估 glifo/parley（ADR-06）。
