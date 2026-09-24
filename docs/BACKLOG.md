# Velm — 待办清单（Backlog）

> 记录 v1 收尾（T16 完成，提交 `704d158`）与 v1.1 增量（提交 `2178c4a`）之后仍未完成的全部工作。
> 与 `docs/SPEC.md`（§12 成功标准）、`docs/PLAN.md`（v1.1 增量记录）、`docs/DECISIONS.md`（ADR）互为对照。
> 更新约定：每完成一项，勾选并在提交信息中引用本文件的编号（如 `B5`）。
>
> 最后更新：2026-09-24（提交 `2178c4a` 之后）

## 状态总览

| 编号 | 事项 | 类别 | 优先级 | host 可验证 | 状态 |
|---|---|---|---|---|---|
| B1 | SC-1~SC-6 真机证据（CP-C/D/E） | 设备依赖 | P0 | 否 | ☐ 未开始 |
| B2 | T14 启停 / 生命周期压测脚本化 | 设备依赖 | P0 | 否 | ☐ 未开始 |
| B3 | T15 release APK 与干净环境复现 | 设备依赖 | P0 | 否 | ☐ 未开始 |
| B4 | 按压 / 禁用态真机观感确认 | 设备依赖 | P1 | 否（逻辑已覆盖） | ☐ 未开始 |
| B5 | `Intent` 执行器 + `SavedInstanceState`（SC-11） | 功能 | **P1（本机最高优先）** | 是 | ☐ 未开始 |
| B6 | 真实文本度量 + 自动换行 + `Choreographer` 动画（SC-13） | 功能 | P1 | 部分是 | ☐ 未开始 |
| B7 | `Image` 真实解码 + `Edit`/IME（SC-14） | 功能 | P1 | 部分是 | ☐ 未开始 |
| B8 | `x86_64` 模拟器目标 + 焦点态 `state_focused`（SC-12） | 功能/平台 | P2 | 部分是 | ☐ 未开始 |
| B9 | margin 精修 / 多密度 Dp 一致性（SC-10） | 功能 | P2 | 是 | ☐ 未开始 |
| B10 | `Card` 阴影参数精修 / 动画转场（SC-15 剩余） | 功能 | P2 | 是 | ☐ 未开始 |
| B11 | crates.io 发布（等 `vello_gpu` 0.2.0 上架 或 fork 改名） | 发布 | P2 | 否 | ☐ 阻塞（上游） |
| B12 | 发布前把 vello 系列依赖从 git 源改回注册表 | 发布 | P2 | 是 | ☐ 未开始 |

---

## A. 设备 / 环境依赖（本机无 adb / 设备 / NDK 运行时，无法推进）

### B1 — SC-1~SC-6 真机证据归档（P0）
- **内容**：在 arm64 真机（minSdk 24）上跑通并留痕：SC-1 产出 `libcounter.so`（`nm -D` 可查 `ANativeActivity_onCreate`）并打 APK 启动；SC-2 生命周期链 + 首帧日志 + 深色背景/计数文本/绿红圆角按钮渲染；SC-3 连续点击 20 次数值准确；SC-4 空白点击无反应、MOVE/UP 不重复触发；SC-5 反复 Home/返回/回前台/旋转各 ≥20 次无崩溃无 ANR；SC-6 退出后引擎线程 join 完成、无 UAF。
- **验收**：SC-1~SC-6 逐项有截图 / logcat / 命令输出归档（写入 `docs/` 或 `scripts/` 结果留痕）。
- **依赖**：设备 + NDK + `cargo-apk2`。
- **备注**：CP-C（静态画面）、CP-D（交互与生命周期）、CP-E（完成评审）均卡在此。

### B2 — T14 启停 / 生命周期压测脚本化（P0）
- **内容**：把 Home/返回/回前台/旋转/挂后台的反复操作脚本化（`scripts/`），跑 ≥20 轮并采集 logcat，验证无 surface / 窗口相关 abort（对应风险 R6）。
- **验收**：脚本可一键复跑，输出无 abort / ANR / UAF 迹象的结果报告。

### B3 — T15 release APK 与干净环境复现（P0）
- **内容**：在干净环境按 `README.md` 命令链从源码走到 release APK 安装运行（SC-9 的强化版）。
- **验收**：干净环境一次复现成功，命令链与文档一致。

### B4 — 按压 / 禁用态真机观感确认（P1）
- **内容**：增量 2（ADR-14）的按压压暗（`PRESSED_SCALE=0.85`）与禁用降透明（`DISABLED_ALPHA=0.5`）在真机上观感确认，并确认「按住不动不卡态」（跟踪逻辑已在 host 覆盖，仅观感需连机）。
- **验收**：真机录屏 / 截图确认；若有观感问题回 DECISIONS 记录调参。

---

## B. v1.1 功能 backlog（优先做本机可验证项）

### B5 — `Intent` 执行器 + `SavedInstanceState`（SC-11）— **本机最高优先**
- **现状**：`crates/velm/src/app/activity.rs` 的 `Intent<Message>` 是纯占位（仅 `PhantomData`，只有 `none()`）；`Activity::SavedInstanceState` 关联类型存在但 `on_create` 恒传 `None`，`update`/`on_create` 返回的 Intent 只被引擎 trace、不执行（SPEC §7.6）。
- **目标**：
  1. `Intent` 可承载后台任务，任务完成后把 `Message` 送回引擎消息队列（需定 executor 与线程模型）；
  2. `SavedInstanceState` 可保存 / 恢复（验收用例：计数值在重建后恢复）。存储方式另议（ADR-01 已移除 `redb`；`serde` 按需重新引入）。
- **验收**：SC-11 用例通过——后台任务消息回投、状态保存/恢复计数。
- **影响面**：`app/activity.rs`、`app/state.rs`、`engine/activity_thread.rs`、`engine/events.rs`。
- **备注**：纯逻辑部分 host 可测；`Send + 'static` 约束已在 §3.3 预留。

### B6 — 真实文本度量 + 自动换行 + 动画（SC-13）
- **内容**：以真实字形度量替换 `chars * size * 0.6` 近似；支持自动换行；`AChoreographer` 驱动的动画 / 转场。
- **验收**：SC-13；多语言（中英）换行正确，动画帧率可接受。
- **影响面**：`render/font.rs`、`layout/measure.rs`、`render/scene.rs`。
- **备注**：字体 fallback 完善度亦属此项。

### B7 — `Image` 真实解码 + `Edit`/IME（SC-14）
- **现状**：`Image` 仅占位色填充；`Edit` 仅静态展示 + 输入拦截骨架。
- **内容**：接入图片解码与资源管线；`Edit` 接入软键盘 / IME 文本输入。
- **验收**：SC-14；真机可显示图片、可输入文本。
- **影响面**：`view/widget.rs`、`render/scene.rs`、`engine/`（IME 事件）。

### B8 — `x86_64` 目标 + 焦点态（SC-12）
- **内容**：`x86_64` 模拟器目标可用；焦点态 `state_focused`（ADR-14 明确列为剩余项）。
- **验收**：SC-12。
- **影响面**：构建配置、`view/params.rs`（`Interaction` 扩 `focused`）、`render/scene.rs`、`engine/hit_test.rs`。

### B9 — margin 精修 / 多密度 Dp 一致性（SC-10）
- **内容**：四方向 margin 与验收用例一致；`Dp` 在多 density 设备上物理尺寸一致。
- **验收**：SC-10；多密度设备对比。
- **备注**：多分辨率真机部分需连机，逻辑部分 host 可测。

### B10 — `Card` 阴影精修 / 动画转场（SC-15 剩余）
- **内容**：`Card` 阴影参数精修；动画 / 转场（ADR-10「v1 不做」的剩余推迟项；按压态已由 ADR-14 完成）。
- **验收**：SC-15 勾选完成。

---

## C. 工程 / 发布

### B11 — crates.io 发布（P2，阻塞于上游）
- **阻塞原因**：`vello_gpu` 0.2.0 未上架 crates.io（仅有 0.1.0 占位），`vello_gpu_shaders` 完全不存在；crates.io **不接受 git / path 依赖**，无 `--force` 开关。
- **两条出路**：
  1. **等上游**：linebender 在 crates.io 发布 `vello_gpu` 0.2.0 后，改回纯 `version` 即可 `cargo publish -p velm`；
  2. **fork 改名发布**：以新 crate 名发布 fork 版本到 crates.io，并相应调整 velm 源码的 `use vello_gpu::` 路径。
- **备注**：当前 vello 系列依赖为 `linebender/vello` git 源（pin rev `9d1eb48`），仅适用于本地 / CI 构建态。

### B12 — 发布前依赖回退（P2）
- **内容**：发布前把根 `[workspace.dependencies]` 7 条与 `crates/velm/Cargo.toml` 3 条 vello 依赖从 git 源改回注册表 `version`（配合 B11）。
- **验收**：`cargo publish -p velm --dry-run` 通过。

---

## 已完成的 v1.1 增量（归档，便于对照）

- **增量 1**：Android 密度模型 + 8 复合组件（ADR-13；提交 `1fb7aaf` + `704d158`）— SPEC §7.10。
- **增量 2**：交互态 `enabled` / `pressed`（ADR-14；提交 `2178c4a`）— SPEC §7.11，SC-15 按压态部分已勾选。

## 追溯

- 成功标准：`docs/SPEC.md` §12（SC-1~SC-15）
- 任务计划：`docs/PLAN.md`（T1~T16 + v1.1 增量记录）
- 决策记录：`docs/DECISIONS.md`（ADR-01~ADR-14）
- 验证脚本：`scripts/`
