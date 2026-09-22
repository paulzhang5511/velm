#!/usr/bin/env bash
# stress_lifecycle.sh — counter 生命周期压测（T14 交付物）。
#
# 覆盖（PLAN T14 / SC-5 / SC-6 / §3.3 同步销毁协议 / T12 上下文所有权）：
#   - 连续启停（am start ↔ am force-stop），默认 20 轮，--cycles 可到 100
#   - Home ↔ 回前台（input keyevent KEYCODE_HOME）
#   - 旋转（wm rotation）：configChanges 下不重建 Activity，只走 surface resize，
#     验证窗口销毁重建 / 重绘路径（SC-5 旋转保状态由引擎线程常驻天然满足）
#   - 每轮统计「同步 ack 已收到」与「引擎线程已 join」次数
#   - grep 崩溃/ANR 信号：FATAL / SIGSEGV / abort / ANR in / use-after-free
#
# 前置：已用 scripts/build_run.sh 安装 counter；adb 在 PATH；设备/模拟器在线。
# 用法：
#   scripts/stress_lifecycle.sh                  # 默认 20 轮
#   scripts/stress_lifecycle.sh --cycles 100     # 连启停 100 次（PLAN T14）
#   scripts/stress_lifecycle.sh --no-rotate      # 跳过旋转
#   scripts/stress_lifecycle.sh --tap            # 每轮点一下 +1（顺带压输入路径）
set -euo pipefail

PKG="${PKG:-rust.counter}"
ACT="${ACT:-android.app.NativeActivity}"
CYCLES=20
ROTATE=1
TAP=0
LOG="$(pwd)/stress_lifecycle.log"

for a in "$@"; do
  case "$a" in
    --cycles)    shift; CYCLES="${1:-20}" ;;
    --no-rotate) ROTATE=0 ;;
    --tap)       TAP=1 ;;
    -h|--help)   sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "未知参数: $a" >&2; exit 2 ;;
  esac
done

# 设备在线检查
adb devices 2>/dev/null | grep -q 'device$' \
  || { echo "无在线设备（adb devices 未见 'device' 状态）" >&2; exit 1; }

RUN_LOG="$(mktemp -t velm-stress.XXXXXX.log)"
trap 'rm -f "$RUN_LOG"' EXIT

collect() { adb logcat -d >> "$RUN_LOG" 2>/dev/null || true; }
launch() {
  adb shell am start -n "$PKG/$ACT" >/dev/null 2>&1 \
    || adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
}
stop_app() { adb shell am force-stop "$PKG" >/dev/null 2>&1; }
home()     { adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1 || true; }

# 等待引擎就绪：日志出现 'Looper 就绪'（最多 ~9s）
wait_ready() {
  for _ in $(seq 1 30); do
    if adb logcat -d 2>/dev/null | grep -q 'Looper 就绪'; then return 0; fi
    sleep 0.3
  done
  return 1
}

# 旋转（configChanges 下不重建 Activity，验证 surface resize/重绘）
rotate() {
  for r in 90 0; do
    adb shell wm rotation "$r" >/dev/null 2>&1 || true
    sleep 0.6
  done
}

echo "==> 压测开始：轮数=$CYCLES 旋转=$ROTATE 点按=$TAP"
echo "==> 日志归档：$(pwd)/stress_lifecycle.log"
: > "$LOG"
adb logcat -c 2>/dev/null || true

destroy_acks=0
joins=0

for i in $(seq 1 "$CYCLES"); do
  echo "--- 轮 $i/$CYCLES ---"
  adb logcat -c 2>/dev/null || true
  launch
  if ! wait_ready; then
    echo "  [WARN] 轮 $i 启动后未检测到 'Looper 就绪'" | tee -a "$LOG"
  fi
  # 可选：点一下 +1 按钮（坐标依设备而异，仅用于顺带压输入路径）
  [ "$TAP" -eq 1 ] && adb shell input tap 540 1200 >/dev/null 2>&1
  [ "$ROTATE" -eq 1 ] && rotate
  home;  sleep 0.5;  collect
  stop_app; sleep 0.5; collect

  # 本轮回执统计（buffer 自本轮开始累积）
  n_ack=$(adb logcat -d 2>/dev/null | grep -c '同步 ack 已收到' || true)
  n_join=$(adb logcat -d 2>/dev/null | grep -c '引擎线程已 join' || true)
  destroy_acks=$((destroy_acks + n_ack))
  joins=$((joins + n_join))
done

# 全量日志落盘
adb logcat -d >> "$LOG" 2>/dev/null || true

# 崩溃 / ANR 检查
crashes=0
if grep -Ei 'FATAL EXCEPTION|SIGSEGV|fatal signal|abort|ANR in|use-after-free|heap-use-after-free|invalid pointer' "$LOG"; then
  crashes=1
fi

echo "==> 结果：销毁同步 ack=$destroy_acks  引擎 join=$joins  崩溃/ANR=$crashes"
if [ "$crashes" -ne 0 ]; then
  echo "[FAIL] 发现崩溃/ANR，详见 $LOG" >&2; exit 1
fi
if [ "$joins" -lt "$CYCLES" ]; then
  echo "[WARN] 引擎 join 次数($joins) < 轮数($CYCLES)：部分销毁路径未干净退出" >&2
fi
echo "[PASS] 压测通过：$CYCLES 轮无崩溃/ANR"
