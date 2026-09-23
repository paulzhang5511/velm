//! Counter 示例应用（cdylib，由 android.app.NativeActivity 加载）。
//!
//! T11：实现框架的 [`Activity`] 契约——竖排一条计数文本 + 「+1」「-1」两个
//! 圆角按钮（ADR-10 视觉基线）。引擎在窗口就绪后按
//! `on_draw → measure_and_layout → render` 出首帧；交互闭环在 T13 接入。

// 导出符号只在 android 目标编译，故 host 构建下 Model / 消息 / 常量只有单测
// 使用——非 android 构建允许 dead_code 以免噪音；android 构建不受影响。
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use velm::{Activity, Color, EdgeInsets, Intent, LayoutDimension, LayoutParams, Orientation, View};

/// ADR-10：「+1」绿。
const GREEN: Color = Color::from_rgba8(0x2E, 0x7D, 0x32, 0xFF);
/// ADR-10：「-1」红。
const RED: Color = Color::from_rgba8(0xC6, 0x28, 0x28, 0xFF);
/// ADR-10：计数文本与按钮文字均为白色。
const WHITE: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);

/// 计数器的两条消息（T13 由点击命中产生）。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Increment,
    Decrement,
}

/// Model：只有一个计数。
struct Counter {
    value: i32,
}

impl Activity for Counter {
    type Message = Msg;
    type SavedInstanceState = ();

    fn on_create(_saved: Option<()>) -> (Self, Intent<Msg>) {
        (Self { value: 0 }, Intent::none())
    }

    fn update(&mut self, message: Msg) -> Intent<Msg> {
        match message {
            Msg::Increment => self.value += 1,
            Msg::Decrement => self.value -= 1,
        }
        Intent::none()
    }

    fn on_draw(&self) -> View<Msg> {
        View::linear_layout(
            Orientation::Vertical,
            vec![
                // 计数文本：28sp 白色，横向铺满、纵向固定 72dp。
                View::text_view(format!("计数：{}", self.value))
                    .set_text_size(28.0)
                    .set_text_color(WHITE)
                    .set_layout_params(LayoutParams {
                        width: LayoutDimension::MatchParent,
                        height: LayoutDimension::Dp(72.0),
                        margin: EdgeInsets::all(24.0),
                    }),
                button("+1", GREEN, Msg::Increment),
                button("-1", RED, Msg::Decrement),
            ],
        )
    }
}

/// 按钮：圆角矩形底色 + 居中文字，点击产生 `message`。
fn button(label: &str, color: Color, message: Msg) -> View<Msg> {
    View::text_view(label)
        .set_text_size(20.0)
        .set_text_color(WHITE)
        .set_background(color, 16.0)
        .set_on_click_listener(message)
        .set_layout_params(LayoutParams {
            width: LayoutDimension::Dp(200.0),
            height: LayoutDimension::Dp(72.0),
            margin: EdgeInsets {
                top: 12.0,
                ..EdgeInsets::all(24.0)
            },
        })
}

/// NativeActivity 入口（native_activity.h 要求的唯一导出符号）。
///
/// # Safety
/// 由 Android 框架调用：`activity` 为有效的 `ANativeActivity*`；
/// `saved_state` 可为空，非空时前 `saved_state_size` 字节可读。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut raw_ndk_sys::ANativeActivity,
    saved_state: *mut std::ffi::c_void,
    saved_state_size: usize,
) {
    // 入口极薄：所有权与逻辑全部在框架内（SPEC §7.7）。
    unsafe {
        velm::engine::run_native_activity::<Counter>(activity, saved_state, saved_state_size)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    use velm::Rect;
    use velm::layout::measure_and_layout;
    use velm::render::build_draw_list;

    /// 在 1080x1920 @ density=2 下布局计数器，返回三个子节点的 rect。
    fn laid_out_children() -> Vec<Rect> {
        let mut root = Counter { value: 0 }.on_draw();
        measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);
        match root {
            View::ViewGroup(group) => group
                .children
                .iter()
                .map(|child| match child {
                    View::TextView(tv) => tv.computed_rect,
                    View::ViewGroup(g) => g.computed_rect,
                    View::Widget(w) => w.common.computed_rect,
                })
                .collect(),
            View::TextView(_) => panic!("根节点应为容器"),
            View::Widget(_) => panic!("根节点应为容器"),
        }
    }

    #[test]
    fn buttons_are_measured_in_dp_and_stacked_vertically() {
        let kids = laid_out_children();
        assert_eq!(kids.len(), 3);

        // 计数文本：margin 24dp*2 = 48px，高 72dp*2 = 144px。
        assert_eq!(kids[0].x, 48.0);
        assert_eq!(kids[0].y, 48.0);
        assert_eq!(kids[0].height, 144.0);

        // 「+1」：子节点 0 推进量 = 144 + 48 + 48 = 240，再 +12dp*2 = 24 → y = 264。
        assert_eq!(kids[1].x, 48.0);
        assert_eq!(kids[1].y, 264.0);
        assert_eq!(kids[1].width, 400.0, "200dp * 2");
        assert_eq!(kids[1].height, 144.0);

        // 「-1」：再推进 144 + 24 + 48 = 216 → y = 480。
        assert_eq!(kids[2].y, 480.0);
    }

    #[test]
    fn laid_out_tree_produces_background_and_text_commands() {
        let mut root = Counter { value: 7 }.on_draw();
        measure_and_layout(&mut root, 1080.0, 1920.0, 2.0);
        let commands = build_draw_list(&root, 2.0);
        // 计数文本 1 条 + 每个按钮「背景 + 文本」各 2 条 = 5 条。
        assert_eq!(commands.len(), 5);
    }

    #[test]
    fn unlaid_out_tree_produces_no_commands() {
        // 未布局时 computed_rect 全零 → 整棵树被跳过（SPEC §7.8 第 4 条）。
        let root = Counter { value: 0 }.on_draw();
        assert!(build_draw_list(&root, 1.0).is_empty());
    }

    #[test]
    fn update_moves_the_counter_into_the_view() {
        let mut counter = Counter { value: 0 };
        counter.update(Msg::Increment);
        counter.update(Msg::Increment);
        counter.update(Msg::Decrement);
        assert_eq!(counter.value, 1);

        match counter.on_draw() {
            View::ViewGroup(group) => match &group.children[0] {
                View::TextView(tv) => assert_eq!(tv.text, "计数：1"),
                View::ViewGroup(_) => panic!("第一个子节点应为文本"),
                View::Widget(_) => panic!("第一个子节点应为文本"),
            },
            View::TextView(_) => panic!("根节点应为容器"),
            View::Widget(_) => panic!("根节点应为容器"),
        }
    }
}
