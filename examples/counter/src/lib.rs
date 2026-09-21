//! Counter 示例应用（cdylib，由 android.app.NativeActivity 加载）。
//!
//! T1 阶段：仅导出 `ANativeActivity_onCreate` 并验证回调链；
//! TEA 计数器逻辑在 T13 接入。
