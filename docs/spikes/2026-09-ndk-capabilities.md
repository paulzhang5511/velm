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

## 4. T3 待验证（未完成）

- [ ] `ALooper_prepare/wake/pollOnce`、`AInputQueue_attachLooper/detachLooper/getEvent/finishEvent` 符号与签名（整数类型以 bindings 为准，不照抄 docs 的 as i32/u32）。
- [ ] `ANativeWindow_acquire/release`、`ANativeWindow_setBuffersGeometry`（RGBA_8888）、width/height 查询。
- [ ] `AConfiguration_fromAssetManager` / `AConfiguration_getDensity`（ADR-12 density）。
- [ ] 引擎线程 Looper 16ms 超时 + 销毁同步 ack POC（SPEC §3.3）。
- [ ] `AChoreographer` 仅登记符号，P1 不接线。
