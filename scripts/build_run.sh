#!/usr/bin/env bash
# build_run.sh — 构建 / 安装 / 启动 counter demo 并跟随日志（T15 交付物）。
#
# 前置（详见 README.md）：
#   - rustup target add aarch64-linux-android （及 x86_64-linux-android）
#   - NDK r27，且导出 ANDROID_NDK_ROOT
#   - cargo install cargo-apk2
#   - adb 在 PATH，且设备/模拟器在线
#
# 用法：
#   scripts/build_run.sh            # release + arm64，构建并安装启动 + 跟随日志
#   scripts/build_run.sh --debug    # debug 构建（含 vello 日志更全）
#   scripts/build_run.sh --x86_64   # x86_64 模拟器目标
#   scripts/build_run.sh --no-run   # 只构建并安装，不启动
#   PKG=rust.counter ACT=android.app.NativeActivity scripts/build_run.sh
set -euo pipefail

cd "$(dirname "$0")/.."   # 切到仓库根

PKG="${PKG:-rust.counter}"
ACT="${ACT:-android.app.NativeActivity}"
TARGET="${TARGET:-aarch64-linux-android}"
MODE="release"           # release | debug
RUN=1

for a in "$@"; do
  case "$a" in
    --debug)   MODE="debug" ;;
    --x86_64)  TARGET="x86_64-linux-android" ;;
    --no-run)  RUN=0 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "未知参数: $a" >&2; exit 2 ;;
  esac
done

: "${ANDROID_NDK_ROOT:?请先设置 ANDROID_NDK_ROOT（如 $HOME/Library/Android/sdk/ndk/27.3.13750724）}"

echo "==> cargo apk2 build ($MODE, $TARGET)"
if [ "$MODE" = "release" ]; then
  cargo apk2 build -p counter --target "$TARGET" --release
else
  cargo apk2 build -p counter --target "$TARGET"
fi

# cargo-apk2 产物路径：target/<profile>/apk/counter.apk（profile=debug|release）
APK="target/$MODE/apk/counter.apk"
if [ ! -f "$APK" ]; then
  APK="$(ls -1 target/*/apk/counter.apk 2>/dev/null | head -n1 || true)"
fi
[ -n "$APK" ] && [ -f "$APK" ] || { echo "未找到 APK（期望 target/$MODE/apk/counter.apk）" >&2; exit 1; }

echo "==> 安装 $APK"
if ! adb install -r -g "$APK" 2>/dev/null; then
  echo "    adb install 失败，回退到 cargo apk2 run"
  cargo apk2 run -p counter --target "$TARGET" $([ "$MODE" = "release" ] && echo --release) || true
fi

[ "$RUN" -eq 0 ] && { echo "==> 已安装（--no-run）"; exit 0; }

echo "==> 启动 $PKG/$ACT"
adb shell am start -n "$PKG/$ACT" >/dev/null 2>&1 \
  || adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1

echo "==> 跟随日志（Ctrl-C 退出）；引擎 tag = VelmEngine"
adb logcat -s VelmEngine AndroidRuntime
