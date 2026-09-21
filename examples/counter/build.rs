//! 打包链接适配（ADR-02/ADR-04）。
//!
//! `raw-ndk-sys` 0.1.2 的 build.rs 会无条件链接 `aaudio`/`amidi`/
//! `neuralnetworks` 等 API 26+ 才提供的 NDK 库，而 cargo 的 build script
//! `-L` 搜索路径不跨 crate 传播；cargo-apk2 又以 minSdk 24 的 sysroot
//! 链接最终 cdylib，导致找不到这些 stub。
//!
//! 本 crate 是最终 cdylib 链接单元，其 build script 输出的 `-L` 直接
//! 作用于最终链接：这里补入 NDK 的 API 29 stub 目录。链接行自带
//! `--as-needed`，未引用任何符号的库不会进入 `DT_NEEDED`（已用
//! readelf 核验产物仅依赖 liblog/libdl/libc），故不影响 minSdk 24 运行时。
//!
//! 上游修复方向：raw-ndk-sys 按目标 API level 条件链接（见 docs/spikes，
//! T3 跟踪）。

use std::path::{Path, PathBuf};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH");
    let triple = match arch.as_str() {
        "aarch64" => "aarch64-linux-android",
        "x86_64" => "x86_64-linux-android",
        other => panic!("velm v1 不支持的 Android 架构: {other}"),
    };

    let prebuilt = ndk_prebuilt_dir(triple)
        .unwrap_or_else(|| panic!("未找到 Android NDK（设置 ANDROID_NDK_ROOT/ANDROID_NDK_HOME）"));

    let lib_dir = prebuilt
        .join("sysroot")
        .join("usr")
        .join("lib")
        .join(triple)
        .join("29");
    assert!(
        lib_dir.exists(),
        "NDK stub 目录不存在: {}",
        lib_dir.display()
    );

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=NDK_HOME");
}

/// 定位 NDK 的 `prebuilt/<host-tag>` 目录。
fn ndk_prebuilt_dir(triple: &str) -> Option<PathBuf> {
    // 1) cargo-apk2 / cargo-ndk 设置 CC_<triple>=.../prebuilt/<host>/bin/clang
    let cc_env = format!("CC_{}", triple.replace('-', "_"));
    if let Some(cc) = std::env::var_os(&cc_env)
        && let Some(prebuilt) = Path::new(&cc).parent().and_then(Path::parent)
        && prebuilt.join("sysroot").is_dir()
    {
        // bin/clang -> bin/.. -> prebuilt/<host>
        return Some(prebuilt.to_path_buf());
    }

    // 2) 显式 NDK 环境变量
    let host_tag = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    for var in ["ANDROID_NDK_ROOT", "ANDROID_NDK_HOME", "NDK_HOME"] {
        if let Ok(root) = std::env::var(var) {
            let p = PathBuf::from(root)
                .join("toolchains")
                .join("llvm")
                .join("prebuilt")
                .join(host_tag);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}
