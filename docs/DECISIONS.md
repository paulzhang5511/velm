# Velm 技术决策记录（ADR）

- **日期**：2026-09-21
- **状态**：已决策，待人类 review（review 后作为 PLAN/IMPLEMENT 的约束基线）
- **关联文档**：`docs/SPEC.md`（§0 假设、§4 技术栈、§14 Open Questions）、`docs/PLAN.md`
- **决策原则**：v1 只交付「零胶水 NativeActivity + TEA + LinearLayout + 文本/点击 + vello 渲染」最小闭环；一切与闭环无关、docs 无需求、增加构建/设备风险的依赖与能力一律推迟。

本机环境基线（已核实）：rustc/cargo 1.95.0（支持 edition 2024）；已装 target `aarch64-linux-android`、`armv7-linux-androideabi`、`x86_64-linux-android`；NDK r26.3 与 r27.3 均在 `~/Library/Android/sdk/ndk/`；已装 `cargo-ndk`，未装 apk/xbuild 类工具。

---

## ADR-01：v1 移除 ort / rstar / redb / jni / image / rayon / serde（对应 Q1）

**决策**

从 `Cargo.toml` 移除以下依赖，v1 框架与 demo 均不使用：`ort`、`rstar`、`redb`、`jni`、`image`、`rayon`、`serde`、`serde_json`。保留 `crossbeam-channel`、`pollster`、`bytemuck`、`log`。

**理由**

- 两份 docs（PRD/架构）对这些 crate **零需求描述**，属于未文档化的方向暗示，不能反向当成需求。
- `ort`（ONNX Runtime）会拉入原生运行时（数十 MB .so）、把 minSdk 抬到 28、显著拖慢交叉编译，与「轻量 UI 框架」定位冲突。
- `jni` 与 PRD G1「零胶水、零 JNI 业务开销」直接冲突；保留它会持续诱导绕过定位。
- `rstar`（空间索引）在 v1 仅数十节点的 View 树中没有消费方（DFS hit-test 足够）；`redb`/`serde*` 在 SavedInstanceState 推迟到 P1 后无用途；`image` 在没有图片视图时无用途；`rayon` 在 v1 单引擎线程模型下无并行任务。
- 依赖越少，android 交叉编译面越小、越容易先跑通闭环（fail fast）。

**否决方案**

- 「先留着以后用」：否决。未使用的依赖是负债而非资产——升级 wgpu/NDK 时它们会成为编译失败面，且模糊框架边界。未来需要时按新 spec 单独引入（均为可逆决策）。

**后续**：若确认产品规划包含 AI 推理/空间检索/本地持久化，各自单开 spec 与模块（如 `velm-ml`、持久化层），不进框架核心。

---

## ADR-02：NDK 绑定保留 raw-ndk-sys 0.1.2（✅ 2026-09-22 T3 实测定稿，对应 Q2）

**决策**

v1 使用 `raw-ndk-sys = "0.1.2"`（Cargo.lock 已验证：自包含、无传递依赖）。**T3 spike 已核对全部所需符号齐备**（Looper/输入队列/窗口/density/触摸，含完整签名与 bindgen 类型差异，见 `docs/spikes/2026-09-ndk-capabilities.md` §4），并经 x86_64/API36 真机闭环验证（引擎线程 Looper、同步销毁 ack、acquire/release、density=1.00、10 次启停 0 ANR）。**不切换 ndk-sys**；`ndk-sys 0.6` 仅作为未来 raw-ndk-sys 停更时的备选，不再作为本项目兜底前置。

**理由**

- docs 全部 FFI 代码按 raw-ndk-sys 的符号风格编写，沿用迁移成本最低。
- raw-ndk-sys 是纯 bindgen 全量绑定、无 jni-sys 等附加依赖（实测 lock 中其 dependencies 为空），符合「零胶水」。
- ndk-sys 0.6（rust-mobile 维护、绑定 NDK r28 头、11769913）更新但属于「sys 绑定」，同样不是胶水层，切换不违反定位；作为确定的兜底。

**否决方案**

- 直接上 `ndk`/`android-activity` 高层 crate：否决，违反零胶水核心定位（SPEC G1、Boundaries Never）。

---

## ADR-03：渲染栈改用 crates.io 正式版 vello 0.10 + wgpu 29，放弃 git 开发快照（对应 Q3）✅ 2026-09-22 T2 spike 四切片全部真机定稿

**决策**

- 放弃现状 `Cargo.toml` 中的 git 源 `vello_gpu`/`vello_common`（rev `5c22cff…`，0.2 开发期拆分形态）。
- 改用 crates.io 正式发布的 **`vello = "0.10"`**（默认启用 wgpu feature），与之统一使用 **`wgpu = "29"`**（vello 0.10 要求 `wgpu ^29.0.3`），`peniko = "0.6"`、`skrifa = "0.44"` 由 vello 带入、必要时在框架中显式声明同版本。
- `raw-window-handle` 维持现状 **0.6**。

**理由（证据）**

- crates.io API 实测 vello 0.10.0（2026-08 发布）依赖：`peniko ^0.6.1`、`skrifa ^0.44.0`、`wgpu ^29.0.3`(optional/default)、`vello_encoding ^0.10`、`thiserror ^2`、`bytemuck ^1.25`——与现状 lock 中 peniko 0.6.1、skrifa 0.44、bytemuck 1.25 完全同代，仅 wgpu 需从 30 降到 29。
- 现状 git 快照的 `vello_common 0.2` 在 crates.io 上已被重新定位（其页面说明 GPU 版 Vello 不使用该 crate，它服务于 Vello CPU 线），说明该拆分形态已被上游重组；固定一个被重组的开发快照等于锁死在无文档、无升级路径的状态。
- 框架渲染器代码为零（docs 中 `vello_renderer.rs` 只有文件名），现在切正式版没有任何迁移成本，反而能拿到完整 docs.rs/示例与稳定 API。
- wgpu 30→29 的降级只影响尚不存在的代码；版本必须与 vello 内部统一，否则 Surface/Device 类型不匹配。

**否决方案**

- pin git rev 继续用 0.2 快照：否决，理由如上（无文档、形态已被上游放弃、无法获得补丁）。
- 强行 wgpu 30 + vello 0.10：否决，semver 大版本不一致会导致类型分裂。

**证据**：<https://crates.io/crates/vello/0.10.0>、<https://crates.io/api/v1/crates/vello/0.10.0/dependencies>、<https://crates.io/crates/vello_common/0.2.0>

**T2 spike 真机定稿（2026-09-22，详见 `docs/spikes/2026-09-render-poc.md`）**：四切片全部通过——① wgpu29 在模拟器 SwiftShader Vulkan 上建 surface/清屏（ranchu debug_utils 缺陷用 `DISCARD_HAL_LABELS` 绕过）；② vello0.10 Scene 经「Rgba8Unorm 中间纹理 + TextureBlitter」上屏，**surface 必须强制非 sRGB `Rgba8Unorm`** 以避免双重 sRGB 编码，limits 用 `Limits::default()`；③ skrifa0.44 直绘中英文（ADR-06）；④ `onNativeWindowResized` 重配 surface + 重建中间纹理，竖↔横旋转铺满、0 崩溃。锁定依赖 `wgpu=29`(vulkan)、`vello=0.10`(wgpu)、`peniko=0.6`、`skrifa=0.44`、`raw-window-handle=0.6`、`pollster=0.4`。

---

## ADR-04：打包链使用 cargo-apk2；cargo-ndk 作为仅产出 .so 的备用（对应 Q4）

**决策**

- v1 开发闭环（编译→打包 APK→安装→logcat）使用 **cargo-apk2（≥1.4）**：无需 Gradle、原生支持以 `NativeActivity` 提供的 cdylib、2026-08 仍活跃维护。
- 已安装的 `cargo-ndk` 保留，用于 CI/只产出 `.so` 的场景；未来需要 AAB/上 Google Play 时再评估 Gradle 或 xbuild。
- Phase 0 spike 先验证 cargo-apk2 对 workspace 中独立 cdylib package（见 ADR-07）的打包与 `AndroidManifest` 元数据配置；若 cargo-apk2 对 workspace 支持有阻塞，回退到「cargo-ndk 产 .so + 最小手写 manifest/aapt 打包」并记录。

**理由（证据）**

- 旧 cargo-apk 最后版本停在 0.10.0（2023-11），其页面明确警告「已被 xbuild 取代」，不可作为新工程基线。
- cargo-apk2 1.4.0（2026-08-27 更新）自我定位即为「cargo-apk 停滞后继任者、最小配置、无 Gradle、尤其适合通过 NativeActivity 提供的应用」，与本项目形态精确匹配。
- xbuild 能力更全（含 Apple/AAB）但概念面更大；Gradle 路线需要 JVM 工程维护，与零胶水/最小闭环目标不符，推迟到有上架需求时。

**证据**：<https://crates.io/crates/cargo-apk2/1.4.0>、<https://crates.org.cn/crates/cargo-apk>（废弃声明）、<https://docs.rs/crate/cargo-xbuild/0.5.34>

---

## ADR-05：v1 手写 LinearLayout 布局，不引入 taffy（对应 Q5）

**决策**

v1 布局为手写 LinearLayout（纵向/横向），实现 SPEC §7.4 的修正版算法（margin 生效、density 换算、容器 WrapContent 按子节点求和）。不引入 taffy；完整 Flexbox（weight/justify/align/grow）列入后续版本，届时再评估 taffy 版本与 `LayoutParams` 扩展。

**理由**：v1 只有顺序排列的文本/按钮；taffy 在现状 lock 中不存在，引入它会新增一个需要适配的布局抽象并推迟闭环；手写算法约一个文件、可 host 单测、与 Android 早期 LinearLayout 心智模型一致。

---

## ADR-06：文本栈定稿为 skrifa 0.44 直绘 + vello draw_glyphs（对应 Q6）✅ 2026-09-22 T2 Slice 3 定稿

**决策**

- v1 必须能渲染**英文与中文**文本（SC-13 中「真实文本度量/中文」提级为 P0 的显示部分；自动换行仍为 P1）。
- **定稿：采用 skrifa 0.44 直绘**（vello 0.10 已内部依赖同一 skrifa，零新增解析栈），自己做 cmap（字符→glyph id）+ 水平 advance 排版，把 `vello::Glyph{id,x,y}` 喂给 `Scene::draw_glyphs(&FontData)`；不引入 glifo/parley。
- 字体来源：Android 系统字体 `/system/fonts/Roboto-Regular.ttf`（拉丁/数字，ttf index 0）与 `/system/fonts/NotoSansCJK-Regular.ttc`（中文 fallback，**ttc index 2 = Noto Sans CJK SC**，实测见 render-poc §4.2）；逐字符 cmap 覆盖做双字体回退分段（`shape_runs`）。系统字体对所有进程可读，且 NotoSansCJK ttc 约 32MB 不应打包进 APK。
- WrapContent 文本宽度：用 skrifa `GlyphMetrics::advance_width` 真实度量替换 SPEC §7.4 的 `chars*size*0.6` 近似（英文按 advance、中文全角约 1em）；T6 布局接入。

**glifo 0.2 时间盒对比结论（为何不选）**

- glifo 0.2（linebender 官方、self-described experimental、0.2→0.3 快速迭代）是 glyph **atlas 光栅化缓存 + DrawSink/GlyphRenderer** 方案，依赖 `vello_common 0.1`、hashbrown、foldhash、smallvec、bytemuck（可选 png），面向 vello **下一代 vello_common/vello_hybrid 架构**；其 renderer 仅 `use vello_common::paint::...`，**不与 vello 0.10 的 `Scene`/`DrawGlyphs` 对接**。
- 在 ADR-03 已锁定并实证 vello 0.10 的前提下，引入 glifo 0.2 需另起 vello_common 0.1 渲染栈、绕过 vello 0.10 Scene，等于推翻 ADR-03 并新增 6+ 依赖；换取的 glyph atlas 缓存、下划线/删除线、富文本能力在 P0 计数器（单行数字 + 中英文标签、无 emoji/复杂连字/BiDi）用不到。
- 触发再评估的条件：未来升级到 vello 新版（vello_common 架构），或需要富文本/多行排版/emoji 时，重新评估 glifo/parley。

**理由（证据）**

- vello 0.10 直接依赖 `skrifa 0.44.0`，且其 `Scene::draw_glyphs` 内部就用 `skrifa::FontRef::from_index(data, index)` 处理 COLR/bitmap glyph——选 skrifa 与 vello 同栈、版本天然对齐。
- T2 Slice 3 真机实证：skrifa cmap + advance + vello draw_glyphs 正确渲染 Roboto「Count: 0」与 Noto SC「计数 +1」（中文简体字形、拉丁回退、基线对齐、28px 白色），3/3 启停干净、0 崩溃；详见 `docs/spikes/2026-09-render-poc.md` §4。

**证据**：T2 Slice 3 真机实证（render-poc §4）；glifo 0.2.0 源码依赖与 renderer 后端；<https://crates.io/crates/glifo/versions>、<https://crates.io/api/v1/crates/vello/0.10.0/dependencies>

---

## ADR-07：Cargo workspace 划分——框架 rlib + 独立 cdylib demo（对应 Q7）

**决策**

根 `Cargo.toml` 转为 workspace 清单：

```text
velm/
├── Cargo.toml            # [workspace] members = ["crates/velm", "examples/counter"]
├── crates/
│   └── velm/             # 框架库：crate-type = ["rlib"]（纯 Rust，供测试/复用）
│       └── src/...
└── examples/
    └── counter/          # Demo 应用：crate-type = ["cdylib"]，依赖 velm
        ├── Cargo.toml
        └── src/lib.rs    # MainActivity + #[no_mangle] ANativeActivity_onCreate
```

- 框架库自身**不**导出 `ANativeActivity_onCreate`（该符号属于应用）；框架提供 `velm::run_native_activity::<A>(...)` 启动函数。
- 移除现状 `src/main.rs`（Hello world 脚手架）。

**理由**：Android NativeActivity 只能加载 cdylib，而框架必须同时可在 host 编译测试（FFI 模块靠 cfg(target_os) 隔离）；单 package 同时承载框架与 demo 会让 rlib/cdylib、host 测试与 C 入口互相污染。workspace 是 android-activity 等同类项目的通行结构。

**否决方案**：demo 放 `examples/` 单文件（cargo examples 是 bin target，无法作为 cdylib 被 NativeActivity 加载）；业务写进框架 lib.rs（docs 现状，无法复用/无法承载多 demo）。

---

## ADR-08：minSdk 24、NDK r27、P0 arm64 + P1 x86_64（对应 Q8）

**决策**

- **minSdk 24**（Android 7.0）：wgpu on Android 走 Vulkan，Vulkan 支持的事实门槛为 API 24；移除 ort 后不再有 api-28 约束。
- NDK 使用本机已装的 **r27.3.13750724**（r26.3 作为备用）。
- ABI：P0 `arm64-v8a`（aarch64-linux-android）；P1 `x86_64`（模拟器调试，target 本机已装）；不做 `armeabi-v7a`（32 位，除非有明确设备需求）。

**理由**：API 24 覆盖绝大多数在网设备且满足 GPU 栈要求；双 ABI 覆盖真机+模拟器；32 位增加 CI 矩阵而无明确收益。

---

## ADR-09：v1 按需渲染 + Looper 唤醒，Choreographer 列 P1（对应 Q9）

**决策**

v1 采用**按需渲染**：仅在状态变更（产生 Message 并 update）、窗口创建/尺寸变化时请求重绘；引擎线程通过 `ALooper_prepare(ALLOW_NON_CALLBACKS)` + `AInputQueue_attachLooper` 由输入事件唤醒，`ALooper_pollOnce` 保留 16ms 超时作为兜底与帧率上限，不做持续 rAF 循环。`AChoreographer_*` 驱动的 vsync 持续渲染（动画前提）列入 P1。

**理由**：UI 框架 v1 无动画，按需渲染省电且模型简单；输入队列 attach 到引擎线程自己的 Looper 后，事件到达即唤醒，不依赖固定睡眠轮询（修正 docs 的 16ms 盲轮询）。

---

## ADR-10：视觉基线——深色背景 + 圆角矩形按钮（对应 Q10）

**决策**

- 窗口清屏色 `#121212`（Material dark surface）。
- demo 按钮为圆角矩形填充 + 居中文字：「+1」绿色（`#2E7D32` 底 / 白字或沿用 docs 的绿字，spike 后按可读性定）、「-1」红色（`#C62828`）；计数文本 28sp 白色；按钮 20sp。
- 矩形同时作为 hit target 的视觉反馈（用户能看到可点区域）；v1 不做按压态动画（P1）。

**理由**：PRD 无视觉稿；纯文本按钮在深色背景上 hit target 不可见，圆角矩形是 vello 最基础能力（filled rounded rect），成本极低且让 SC-3 可肉眼验收。

---

## ADR-11（衍生）：错误处理与 FFI 安全边界

**决策**

- 框架定义 `velm::Error`（`thiserror`，lock 中已有 thiserror 2.x）：覆盖 NDK 返回码、surface/device 创建失败、surface lost/outdated；可恢复错误（surface 失效）重配重试，不可恢复错误记录后停止引擎线程。
- 所有 C 回调入口与引擎线程主函数用 `std::panic::catch_unwind` 包裹：panic 信息经 logcat error 输出后走受控退出（继续运行会破坏 FFI 不变量）；回调中禁止 `unwrap/expect`（入口空指针断言保留 assert，属编程失败，接受 abort 语义）。
- `ANativeWindow` 所有权：回调交付窗口时 `ANativeWindow_acquire`，销毁同步点确认渲染器停止访问后 `ANativeWindow_release`（SPEC §3.3 同步协议的具体实现手段）。

## ADR-12（衍生）：density 与坐标系

**决策**

- density 通过 `AConfiguration_fromAssetManager(activity->assetManager)` + `AConfiguration_getDensity` 获取（dpi/160.0）；拿不到时兜底 1.0 并打一次 warn（ADR-02 spike 验证 raw-ndk-sys 符号）。
- 全链路统一**物理像素绝对坐标**：NDK MotionEvent 的 x/y 本就是像素；布局输出像素；Dp/sp 乘 density。hit-test 前不做额外换算。

---

## ADR-13：Android 风格密度模型（DisplayMetrics）与复合组件收敛（View::Widget + WidgetKind）

**背景**：v1.0 冻结后，用户要求「参考 Android 开发多个组件，考虑 Android 的 density」扩展框架。原 ADR-12 只把 density 当裸 `f32`（dpi/160），且 §7.2 的 `View` 仅有 `TextView` / `ViewGroup`。

**决策**

- **密度模型**：新增 `platform::DisplayMetrics`（host 可测），把 density 升级为 Android 式完整概念——`density`（dpi/160）、`font_scale`（系统字体缩放，默认 1.0）、`scaled_density = density × font_scale`、`DensityBucket`（ldpi~xxxhdpi 六桶 + `Other`）。**dp 与 sp 分离**：布局尺寸走 dp（× density），文本字号走 sp（× scaled\_density）；所有最终物理像素**四舍五入取整**（对齐 `TypedValue.complexToDimensionPixelSize`）。换算必须经 `DisplayMetrics`，禁止散落 `value * density`。本 ADR **扩展** ADR-12（坐标系与观测路径不变）。
- **复合组件收敛**：`View<Msg>` 只增加**一个**变体 `Widget(WidgetView<Msg>)`，八个组件（Button / Card / Image / Progress / Check / Switch / Space / Edit）的差异收敛到 `WidgetKind` 枚举；`measure` / `place` / `scene` / `hit_test` 各只需一个 `Widget` 派发分支，避免 `View` 枚举随组件数线性膨胀、以及所有既有穷举 `match` 被迫加 N 个分支。
- **padding 与 Stroke**：`TextView` / `ViewGroup` / `Widget` 三类节点统一支持 `padding: EdgeInsets`（dp）与 `Stroke { color, width_dp }`；新增 `DrawCommand::StrokeRect`，渲染器用 kurbo `Stroke` + `Scene::stroke_rect` / `stroke_path` 实现。
- **向后兼容**：`measure_and_layout` / `build_draw_list` 旧签名保留为薄包装（内部 `DisplayMetrics::new`，font\_scale = 1.0），引擎改走 `*_with(&DisplayMetrics)`。既有 25+ 处调用与测试零改动。

**理由**：Android 的 density 体系（dp / sp / 桶 / 取整）是跨设备一致的既定心智模型，直接复用可让布局与真机对齐；组件以「单变体 + kind 枚举」承载，是把「开放组件集」限制在单点扩展的工程手段（对既有 `match` 的影响为常数级）。

**否决方案**：① 为每个组件加一个 `View` 变体——否决，会让 `View` 枚举与全部 `match` 随组件数线性膨胀；② 把 `font_scale` 折进 `density`——否决，dp 与 sp 必须可分辨（系统字体缩放不应改变布局尺寸）。

**落地**：提交 `1fb7aaf`（24 files, +1620/-158）；详见 SPEC §7.10、`tests/display_metrics.rs`、`tests/widget.rs`。门禁：host test 全绿、双 target clippy `-D warnings` 零警告、`aarch64-linux-android` 交叉编译通过。

**待办（P1）**：SPEC §12 SC-14 / SC-15（图片解码管线、Edit/IME、按压态与动画）。

---

## ADR-14：交互态 `enabled` / `pressed`（补齐 ADR-10 的按压态推迟项）

**背景**：ADR-10 把「按压态」明确列为 v1 之外的推迟项（`v1 不做按压态动画（P1）`）。本增量在不引入动画的前提下补齐**静态**交互态，对齐 Android `View.setEnabled` / `state_enabled` / `state_pressed`，并作为 SPEC §12 SC-15 的首块。

**决策**

- **数据结构**：新增 `view::Interaction { enabled: bool, pressed: bool }`（默认 `enabled = true` / `pressed = false`），挂在 `TextView` / `ViewGroup` / `CommonStyle` 的 `interaction` 字段；不引入新的 `View` 变体（与 ADR-13 的收敛原则一致）。
- **颜色变换集中在纯函数**：`Interaction::tint_fill`（禁用降透明 **且** 按下压暗）与 `tint_content`（仅禁用降透明）。常量 `DISABLED_ALPHA = 0.5`（对齐 Android `DISABLED_ALPHA`）、`PRESSED_SCALE = 0.85`。`enabled` 优先于 `pressed`。颜色计算与几何分离——禁用 / 按下**不改变布局**。
- **命中测试**：禁用节点的 `node_listener` 返回 `None`，即对点击**透明**（事件穿透到下层兄弟）；**只影响本节点、不向下传播**（与 Android `View.setEnabled` 的 base 语义一致，不做 ViewGroup 递归传播）。
- **按压态跟踪放 host 可测层**：`engine::hit_test::{set_pressed_at, clear_pressed}` 复用与命中测试相同的遍历顺序（子节点逆序、闭区间），返回「是否变化」供引擎决定是否重绘。仅**可交互**（绑定监听且未禁用）节点获得按压反馈。
- **引擎接线最小化**：`engine/activity_thread.rs` 的 `handle_motion` 中，`ACTION_DOWN` → `set_pressed_at(true)`、`ACTION_UP/CANCEL` → `clear_pressed()`，有变化才 `note_message`（请求重绘）；不改变既有的「DOWN 命中派发消息」逻辑（FR-I1/I3）。

**理由**：静态两态（可用 / 按下）用「颜色变换纯函数 + 单字段状态」即可覆盖绝大多数视觉反馈需求，无需动画系统（动画仍列 P1）。把状态跟踪放在 host 可测的 `hit_test` 层，使设备侧只剩极少接线，符合本项目「逻辑在 host 可测、FFI 边界最小」的一贯切分。

**否决方案**：① 为交互态引入动画 / `Choreographer`——否决，超出静态反馈所需、且属 P1；② 禁用态向下递归传播到子节点——否决，与 Android base `View` 语义不符，且会让「禁用整块布局」变得不可预期；③ 把 `pressed` 只做成渲染开关、不做状态跟踪——否决，那样引擎无法在 `UP/CANCEL` 时清除，会「卡在按下态」。

**落地**：SPEC §7.11；`tests/interaction.rs`（12 例）。门禁：host test 全绿、双 target clippy `-D warnings` 零告警、aarch64 交叉编译通过。

**待办（P1）**：焦点态（`state_focused`）、动画 / 转场。

---

## 决策汇总表（SPEC §14 关闭对照）

| Q | 议题 | 决策 | ADR |
|---|---|---|---|
| Q1 | ort/rstar/redb/jni 等 | v1 全部移除 | ADR-01 |
| Q2 | NDK 绑定 | raw-ndk-sys 0.1.2 + spike 验符号，兜底 ndk-sys 0.6 | ADR-02 |
| Q3 | vello 来源/版本 | crates.io vello 0.10 + wgpu 29，弃 git 快照 | ADR-03 |
| Q4 | 打包链 | cargo-apk2（无 Gradle），cargo-ndk 备用 | ADR-04 |
| Q5 | 布局引擎 | 手写 LinearLayout，不引 taffy | ADR-05 |
| Q6 | 文本栈 | **定稿 skrifa 0.44 直绘 + vello draw_glyphs**（glifo 面向 vello_common 新栈，不适用），系统字体 Roboto+NotoSC(ttc#2)，中英必达 | ADR-06 |
| Q7 | crate 划分 | workspace：crates/velm(rlib) + examples/counter(cdylib) | ADR-07 |
| Q8 | minSdk/ABI/NDK | minSdk 24、NDK r27、arm64 P0 / x86_64 P1 | ADR-08 |
| Q9 | 帧率模型 | 按需渲染 + Looper 唤醒；Choreographer P1 | ADR-09 |
| Q10 | 视觉基线 | #121212 背景 + 圆角矩形按钮 | ADR-10 |
| — | 错误/FFI 安全 | thiserror 领域错误 + catch_unwind + acquire/release | ADR-11 |
| — | density/坐标 | AConfiguration_getDensity，全链路像素坐标 | ADR-12 |
| — | 密度模型 + 复合组件 | `DisplayMetrics`（dp/sp 分离、像素四舍五入取整）+ `View::Widget` / `WidgetKind` 八组件（Button/Card/Image/Progress/Check/Switch/Space/Edit），padding / Stroke 统一支持 | ADR-13 |
| — | 交互态 | `Interaction{enabled,pressed}` + `set_enabled/set_pressed`；禁用降透明且点击透明（不向下传播），按下背景压暗；`set_pressed_at/clear_pressed` 引擎跟踪 | ADR-14 |
