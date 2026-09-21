# Spike：NDK 能力与打包链实证（T3 / T1）

- 日期：2026-09-21
- 状态：**T1 打包链部分已实证完成**；Looper/输入/窗口同步部分待 T3 继续
- 环境：macOS（Apple Silicon）、NDK r27.3.13750724、rustc 1.95.0（edition 2024）、cargo-apk2 1.4.1、模拟器 x86_64 / API 36

## 1. raw-ndk-sys 0.1.2 实测结论

1. 该 crate 为 NDK r27.3.13750724 的预生成 bindgen 绑定（`src/bindings.rs`，约 1.17 万行），**编译期不需要 NDK**（除非开 `regenerate` feature）。
2. build.rs 在 android target 无条件链接：`android`、`log`、`jnigraphics`、`mediandk`、`aaudio`、`amidi`、`camera2ndk`、`neuralnetworks`。
   - 其中 `aaudio`（API 26）、`neuralnetworks`（API 27）、`amidi`（API 29）在 minSdk 24 sysroot 中无 stub，裸 API24 linker 报 `unable to find library`。
   - build.rs 仅在检测到 `ANDROID_NDK_HOME`/`NDK_HOME` 时追加 `sysroot/usr/lib/<triple>/29` 搜索路径；且 build script 的 `-L` **不跨 crate 传播**到最终 cdylib 链接。
3. 解法（已落地）：
   - 裸 `cargo build --target`：`.cargo/config.toml` 为两个 android target 设 API24 linker + rustflags `-L <sysroot>/usr/lib/<triple>/29`。
   - cargo-apk2：它用 `CARGO_ENCODED_RUSTFLAGS` 覆盖 config rustflags，改由**最终 cdylib**（`examples/counter/build.rs`）从 `CC_<triple>` 推导 NDK prebuilt 目录并输出 `cargo:rustc-link-search`（同 crate 的 -L 对最终链接生效）。
   - 链接行自带 `--as-needed`，未引用符号的高版本库不进入 `DT_NEEDED`。readelf 实证 debug `libcounter.so` 仅依赖 `liblog.so`/`libdl.so`/`libc.so`，minSdk 24 运行时安全。
4. 回调类型实测（bindings 行 4869+）：
   - `ANativeActivity` 字段：`callbacks: *mut ANativeActivityCallbacks`、`vm/env/clazz`、`sdkVersion: i32`、`instance: *mut c_void`、`assetManager: *mut AAssetManager` 等。
   - `ANativeActivityCallbacks` 为 `#[repr(C)]` 的 `Option<unsafe extern "C" fn ...>` 字段集；5 个必绑回调签名与 docs 一致：
     - `onNativeWindowCreated/Destroyed(*mut ANativeActivity, *mut ANativeWindow)`
     - `onInputQueueCreated/Destroyed(*mut ANativeActivity, *mut AInputQueue)`
     - `onDestroy(*mut ANativeActivity)`
5. Rust 2024 注意：`#[no_mangle]` 必须写作 `#[unsafe(no_mangle)]`；`unsafe fn` 体内解引用裸指针需显式 `unsafe {}` 块（否则 clippy `-D warnings` 失败）。

## 2. cargo-apk2 1.4.1 实测结论（ADR-04 落地）

- 子命令：`cargo apk2 check | build | run | gdb`（不是 `cargo apk`）。
- 需要环境变量 `ANDROID_NDK_ROOT`；SDK 默认从 `~/Library/Android/sdk` 探测（亦可设 `ANDROID_SDK_ROOT`）。
- workspace 下需 `-p counter` 指定 cdylib package；`--target <triple>` 指定 ABI。
- metadata（已写入 `examples/counter/Cargo.toml`）：
  ```toml
  [package.metadata.android]
  min_sdk_version = 24
  target_sdk_version = 35   # 跟随本机已装 platform；装 android-36 后可升 36
  [package.metadata.android.application]
  label = "Velm Counter"
  [[package.metadata.android.application.activity]]
  name = "android.app.NativeActivity"
  [[package.metadata.android.application.activity.meta_data]]
  name = "android.app.lib_name"
  value = "counter"
  [[package.metadata.android.application.activity.intent_filter]]
  actions = ["android.intent.action.MAIN"]
  categories = ["android.intent.category.LAUNCHER"]
  ```
  注意 `meta_data`/`intent_filter` 都是 **array of tables（`[[...]]`）**，写成 `[...meta_data]` 会报 "invalid type: map, expected a sequence"。
- 生成产物：`target/debug/apk/counter.apk`，包名 `rust.counter`，自动用 `~/.android/debug.keystore` 签名。
- 实测 manifest：NativeActivity、`lib_name=counter`、MAIN/LAUNCHER、`exported=true`、minSdk 24 均正确；lib 目录 `lib/x86_64/libcounter.so`（16MB debug，release 待 T15）。
- cargo-apk2 默认给 NativeActivity 注入 `configChanges=0x4a0`（orientation|keyboardHidden|screenSize）：**旋转不重建 Activity**，只走 surface resize/configuration 回调。SC-5 旋转压测因此针对窗口销毁重建路径（T14 需绑 `onNativeWindowResized/RedrawNeeded` 或验证 surface 重建）。

## 3. 运行时实证（x86_64 / API 36 模拟器）

正常日志链（tag `VelmEngine`，均在主线程）：

```
ANativeActivity_onCreate: saved_state_size=0
已绑定 5 个生命周期回调
onInputQueueCreated
onNativeWindowCreated
```

- Home：`onNativeWindowDestroyed`；回前台：`onNativeWindowCreated`（surface 销毁/重建链正确）。
- **关键风险实证**：未调用 `AInputQueue_attachLooper` 时，BACK 等按键事件 5001ms 无人 finish → InputDispatcher 报 ANR。
  - 结论：输入消费（T12）落地前 demo 不可交互，这是预期；T3 spike 必须最先验证 `ALooper_prepare(ALLOW_NON_CALLBACKS)` + `AInputQueue_attachLooper` + `getEvent`/`finishEvent` 闭环，且每事件恰好 finish 一次。
  - `onInputQueueDestroyed`、`onDestroy` 的干净路径在 T12/T14 随引擎线程验证（BACK 键当前被 ANR 对话框拦截，无法用于验证）。

## 4. 符号核对清单（2026-09-22 完成，raw-ndk-sys 0.1.2 全部具备 → ADR-02 定稿，不切 ndk-sys）

| 符号 | 签名要点（bindings 实测） |
|---|---|
| `ALooper_prepare` | `(opts: c_int) -> *mut ALooper`；`ALOOPER_PREPARE_ALLOW_NON_CALLBACKS = 1` |
| `ALooper_forThread` | `() -> *mut ALooper` |
| `ALooper_wake` | `(looper)` |
| `ALooper_pollOnce` | `(timeoutMillis: c_int, *mut c_int, *mut c_int, *mut *mut c_void) -> c_int`；返回 `WAKE=-1 / CALLBACK=-2 / TIMEOUT=-3 / ERROR=-4 / >=0 为 ident`（**所有返回值都可能隐含 WAKE**） |
| `AInputQueue_attachLooper` | `(queue, looper, ident: c_int, callback: Option<fn> = null, data: *mut c_void)`；用 ident=1、callback=null，pollOnce 返回 ident 后 getEvent |
| `AInputQueue_detachLooper` | `(queue)`（同步：返回后不再有事件回调） |
| `AInputQueue_hasEvents / getEvent` | `getEvent(queue, *mut *mut AInputEvent) -> i32`（<0 无事件/错误） |
| `AInputQueue_preDispatchEvent` | `(queue, event) -> i32`（非 0 表示已被 IME 预派发，须放弃本轮处理） |
| `AInputQueue_finishEvent` | `(queue, event, handled: c_int)`——getEvent 后必须恰好一次 |
| `AInputEvent_getType` | `(*const AInputEvent) -> i32`；`AINPUT_EVENT_TYPE_KEY=1 / MOTION=2` |
| `AKeyEvent_getKeyCode` | `(*const) -> i32`；`AKEYCODE_BACK = 4` |
| `AMotionEvent_getAction` | `(*const) -> i32`（**i32，非 u32**；低 8 位 action，`& 0xff` 时按 u32 解释；DOWN=0/UP=1/MOVE=2/CANCEL=3） |
| `AMotionEvent_getX/getY` | `(*const, pointer_index: usize) -> f32`；`getPointerCount(*const) -> usize` |
| `ANativeWindow_acquire/release` | `(*mut ANativeWindow)` |
| `ANativeWindow_getWidth/getHeight` | `(*mut) -> i32` |
| `ANativeWindow_setBuffersGeometry` | `(window, w: i32, h: i32, format: i32) -> i32`；`WINDOW_FORMAT_RGBA_8888 = 1` |
| `AConfiguration_fromAssetManager` | `(out: *mut AConfiguration, am: *mut AAssetManager)` |
| `AConfiguration_getDensity` | `(*mut) -> i32`（dpi 原始值，/160.0 得 density） |
| `AConfiguration_delete` | `(*mut AConfiguration)` |

## 5. T3 POC 验证结果（Slice 2，2026-09-22，x86_64/API36 真机实证）

引擎线程 + Looper + 输入队列闭环已实现于 `crates/velm/src/engine/activity_thread.rs`：

- [x] 引擎线程（命名 `velm-engine`）内 `ALooper_prepare(ALLOW_NON_CALLBACKS)`，`AInputQueue_attachLooper(ident=1, callback=None)`；`pollOnce(16ms)` 返回 ident 后 `getEvent → preDispatchEvent → finishEvent` 闭环。
- [x] 主线程回调只经 crossbeam-channel 发布；控制消息入队后 `ALooper_wake`（Looper 指针由引擎线程 prepare 后经 `AtomicPtr` 注册）。
- [x] QueueDestroyed 同步 ack：主线程建一次性 channel 发送并 `recv()` 阻塞，引擎线程 `detachLooper` 后回执，实测 2–4ms 放行。
- [x] BACK 键（keyCode=4）引擎线程消费并 `finishEvent(handled=0)` 交回框架默认 → Activity finish → window/queue destroyed（同步 ack）→ onDestroy 发 Quit → 引擎线程退出 → `join()` 完成；**无 ANR**（对照：T1 未 attach 时 5001ms 必 ANR）。
- [x] `adb shell input tap 160 320` 收到 MotionEvent DOWN(0)/UP(1)，坐标精确，`finishEvent(handled=1)`。
- [x] bindgen 类型注意：`AINPUT_EVENT_TYPE_*` 常量生成为 **u32**，而 `AInputEvent_getType` 返回 i32，比较须 `as i32`；`AMotionEvent_getAction` 返回 **i32**，低 8 位 `& 0xff` 为 action。

### 5.1 关键发现：不提交首帧则触摸事件不投递（T10 硬约束）

未绘制任何 buffer 时（T1/T3-Slice2 早期）：

- `dumpsys window`：window frame 全屏 `[0,0][320,640]`，但 **surface geometry 为 `[0,0][0,0]`**；
- `dumpsys input`：app 的 InputWindowHandle **frame=`[0,0][0,0]`、touchableRegion=`<empty>`**；
- InputDispatcher 静默丢弃触摸（仅见 ActivityRecordInputSink 的 `NO_INPUT_CHANNEL` 提示，该提示本身正常）；按键走焦点窗口通道不受影响（故 BACK 能到 native、触摸不能）。

POC 验证：在 `onNativeWindowCreated` 中 `setBuffersGeometry(RGBA_8888)` + `ANativeWindow_lock` 填 #121212 + `unlockAndPost` 提交一帧（320×640 stride=320）后，InputWindowHandle 几何随即建立，触摸事件立即到达。

**结论（写入 T10 验收）**：渲染器必须在窗口创建后尽快提交首帧（哪怕清屏），否则任何触摸交互都不会被 InputDispatcher 投递；T11 静态画面里程碑天然满足该条件。该 probe 帧为临时代码（`post_probe_frame`），T10 vello 渲染器接入后删除。

## 6. T3 POC 验证结果（Slice 3，2026-09-22）

- [x] 窗口所有权：`onNativeWindowCreated` 中 `ANativeWindow_acquire` 后经 channel 移交引擎线程；真机宽高 **320×640**；density 路径 `AConfiguration_new → fromAssetManager(activity.assetManager) → getDensity → delete`，实测 dpi=160 → **density=1.00**（异常值 0/65534/65535 兜底 1.0 + warn）。
- [x] `onNativeWindowDestroyed` 同步 ack：引擎线程 `ANativeWindow_release` 后回执，主线程才返回（实测 3–33ms）；acquire/release 一一配对（send 失败路径回滚 release；Quit 防御性 release）。
- [x] 反复启停 **10 次（start→tap→BACK）10/10 干净 join，0 ANR、0 FATAL**。
- [x] Home/回前台：Home 只触发 window destroyed（release+ack），**input queue 不销毁、保持 attached**；回前台新 window created 并重新提交首帧，触摸立即恢复；BACK 退出时双资源（window/queue）同步销毁 + join 干净。
- [ ] `AChoreographer` 仅登记符号，P1 不接线（T3 不做）。

## 7. 对后续任务的约束（T3 结论）

1. **T10 渲染器**：窗口创建后必须尽快 queue 首帧（含清屏），否则 InputWindowHandle 几何为 0、触摸不投递（§5.1）；surface 使用全部在引擎线程，主线程仅 acquire/发布/同步 ack。
2. **T5 事件**：常量比较注意 bindgen 类型（`AINPUT_EVENT_TYPE_*` 为 u32，getter 返回 i32）；`AMotionEvent_getAction` 为 i32，低 8 位 action。
3. **T12 引擎线程**：现有 Looper/channel/ack/join 骨架直接演进为正式事件状态机；销毁同步协议（window、queue 各一次 ack）已定型。
4. **T14 压测**：Home 路径不重建 queue，旋转因 configChanges 不重建 Activity（T1 实证），压测脚本以 BACK 启停 + Home/回前台为主。
