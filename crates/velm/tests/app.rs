//! tests/app.rs — Activity 契约与 TEA 运行时（SPEC §7.6）。
//!
//! 覆盖：占位 Intent 的形状、Activity trait 可实现性、`ActivityRuntime` 的
//! 入队 / 消费 / 重绘标志、触控拦截（FR-I2）与「一帧最多重建一次视图树」。

use velm::app::state::ActivityRuntime;
use velm::app::{Activity, Intent};
use velm::event::{MotionEvent, TouchAction};
use velm::view::View;

/// 测试用消息。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Increment,
    Decrement,
}

/// 最小可用 Activity：计数器 + 可开关的触控拦截。
struct Counter {
    value: i32,
    /// 为 `true` 时 `on_touch_event` 拦截并返回 `Increment`。
    intercept: bool,
}

impl Activity for Counter {
    type Message = Msg;
    type SavedInstanceState = ();

    fn on_create(_saved: Option<()>) -> (Self, Intent<Msg>) {
        (
            Self {
                value: 0,
                intercept: false,
            },
            Intent::none(),
        )
    }

    fn update(&mut self, message: Msg) -> Intent<Msg> {
        match message {
            Msg::Increment => self.value += 1,
            Msg::Decrement => self.value -= 1,
        }
        Intent::none()
    }

    fn on_draw(&self) -> View<Msg> {
        View::text_view(format!("count={}", self.value))
    }

    fn on_touch_event(&mut self, _event: &MotionEvent) -> Option<Msg> {
        self.intercept.then_some(Msg::Increment)
    }
}

fn down(x: f32, y: f32) -> MotionEvent {
    MotionEvent {
        action: TouchAction::ActionDown,
        x,
        y,
    }
}

// ── Intent 占位 ─────────────────────────────────────────────────────────────

#[test]
fn intent_none_is_constructible_and_copyable() {
    let intent: Intent<Msg> = Intent::none();
    let copied = intent;
    assert_eq!(format!("{copied:?}"), "Intent::none()");
    assert_eq!(format!("{:?}", Intent::<Msg>::default()), "Intent::none()");
    // 显式走一遍 Clone（Copy 语义下 `let copied = intent` 不触发 clone 实现）。
    // 用 UFCS 而非 `intent.clone()`：后者会触发 clippy::clone_on_copy。
    let cloned = Clone::clone(&intent);
    assert_eq!(format!("{cloned:?}"), "Intent::none()");
}

/// 不覆写 `on_touch_event` 的 Activity：走 trait 默认实现（恒不拦截）。
struct Passive;

impl Activity for Passive {
    type Message = Msg;
    type SavedInstanceState = ();

    fn on_create(_saved: Option<()>) -> (Self, Intent<Msg>) {
        (Self, Intent::none())
    }

    fn update(&mut self, _message: Msg) -> Intent<Msg> {
        Intent::none()
    }

    fn on_draw(&self) -> View<Msg> {
        View::text_view("passive")
    }
}

#[test]
fn default_on_touch_event_does_not_intercept() {
    let mut runtime = ActivityRuntime::new(Passive::on_create(None).0);
    assert!(
        !runtime.on_touch_event(&down(10.0, 10.0)),
        "trait 默认实现恒不拦截"
    );
    assert_eq!(runtime.pending(), 0);
}

// ── Activity 契约 ───────────────────────────────────────────────────────────

#[test]
fn activity_on_create_and_update_are_wired() {
    let (mut counter, _intent) = Counter::on_create(None);
    assert_eq!(counter.value, 0);

    counter.update(Msg::Increment);
    counter.update(Msg::Increment);
    counter.update(Msg::Decrement);
    assert_eq!(counter.value, 1);
}

// ── 运行时：消息与重绘 ──────────────────────────────────────────────────────

#[test]
fn runtime_starts_without_draw_request() {
    let runtime = ActivityRuntime::new(Counter::on_create(None).0);
    assert!(
        !runtime.needs_draw(),
        "首帧由窗口创建事件触发，不由运行时触发"
    );
    assert_eq!(runtime.pending(), 0);
}

#[test]
fn enqueue_does_not_update_until_drain() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    runtime.enqueue(Msg::Increment);
    runtime.enqueue(Msg::Increment);

    assert_eq!(runtime.pending(), 2);
    assert_eq!(runtime.activity().value, 0, "入队不得立即 update");
    assert!(!runtime.needs_draw());

    assert_eq!(runtime.drain(), 2);
    assert_eq!(runtime.activity().value, 2);
    assert_eq!(runtime.pending(), 0);
    assert!(runtime.needs_draw());
}

#[test]
fn drain_without_messages_does_not_request_redraw() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    assert_eq!(runtime.drain(), 0);
    assert!(!runtime.needs_draw(), "空队列不得触发重绘（FR-I4）");
}

#[test]
fn draw_request_is_cleared_once_taken() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    runtime.enqueue(Msg::Increment);
    runtime.drain();

    assert!(runtime.take_draw_request());
    assert!(!runtime.needs_draw());
    assert!(!runtime.take_draw_request());
}

// ── 触控拦截 ────────────────────────────────────────────────────────────────

#[test]
fn touch_interception_enqueues_message() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    runtime.activity_mut().intercept = true;

    // 返回 true：引擎据此跳过默认 hit-test（FR-I2）。
    assert!(runtime.on_touch_event(&down(10.0, 10.0)));
    assert_eq!(runtime.pending(), 1);
    assert_eq!(runtime.drain(), 1);
    assert_eq!(runtime.activity().value, 1);
}

#[test]
fn touch_without_interception_produces_nothing() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    assert!(!runtime.on_touch_event(&down(10.0, 10.0)));
    assert_eq!(runtime.pending(), 0);
    assert!(!runtime.needs_draw());
}

// ── 视图树 ──────────────────────────────────────────────────────────────────

#[test]
fn view_reflects_current_model() {
    let mut runtime = ActivityRuntime::new(Counter::on_create(None).0);
    runtime.enqueue(Msg::Increment);
    runtime.drain();
    runtime.take_draw_request();

    // 引擎每帧最多调用一次 view()（§7.7 禁止一触重建两次）。
    let view = runtime.view();
    match view {
        View::TextView(tv) => assert_eq!(tv.text, "count=1"),
        View::ViewGroup(_) => panic!("期望 TextView"),
        View::Widget(_) => panic!("期望 TextView"),
    }
}

#[test]
fn create_helper_uses_on_create() {
    let runtime = ActivityRuntime::<Counter>::create();
    assert_eq!(runtime.activity().value, 0);
}
