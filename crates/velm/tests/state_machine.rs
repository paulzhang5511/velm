//! tests/state_machine.rs — 引擎生命周期状态机（SPEC §3.4）。
//!
//! 覆盖：窗口 / 队列任意到达顺序、销毁后重建、无窗口时消息与重绘丢弃、
//! 窗口销毁清除未消费的重绘请求、resize 语义、Quit 的资源回收与「之后不再
//! 产生动作」。

use velm::engine::events::{EngineAction, EngineEvent, EngineState, Viewport, step};

const VP: Viewport = Viewport {
    width: 1080,
    height: 1920,
    density: 2.0,
};

fn created() -> EngineState {
    let mut state = EngineState::new();
    let actions = step(&mut state, EngineEvent::WindowCreated(VP));
    assert_eq!(actions, vec![EngineAction::CreateSurface]);
    state
}

// ── 到达顺序：窗口与队列互相独立 ────────────────────────────────────────────

#[test]
fn window_before_queue() {
    let mut state = EngineState::new();
    assert_eq!(
        step(&mut state, EngineEvent::WindowCreated(VP)),
        vec![EngineAction::CreateSurface]
    );
    assert_eq!(
        step(&mut state, EngineEvent::QueueCreated),
        vec![EngineAction::AttachQueue]
    );
    assert!(state.has_window());
    assert!(state.has_queue());
}

#[test]
fn queue_before_window() {
    // docs 隐式假设窗口先于队列；规格要求顺序任意。
    let mut state = EngineState::new();
    assert_eq!(
        step(&mut state, EngineEvent::QueueCreated),
        vec![EngineAction::AttachQueue]
    );
    assert!(!state.has_window());
    assert_eq!(
        step(&mut state, EngineEvent::WindowCreated(VP)),
        vec![EngineAction::CreateSurface]
    );
    assert!(state.has_window());
}

#[test]
fn queue_works_without_window() {
    // 无窗口时输入仍然可以 attach（只是消息会被丢弃）。
    let mut state = EngineState::new();
    step(&mut state, EngineEvent::QueueCreated);
    assert_eq!(step(&mut state, EngineEvent::Message), Vec::new());
    assert!(!state.needs_draw());
}

// ── 无窗口时的丢弃策略 ──────────────────────────────────────────────────────

#[test]
fn message_without_window_is_dropped() {
    let mut state = EngineState::new();
    assert_eq!(step(&mut state, EngineEvent::Message), Vec::new());
    assert!(!state.needs_draw(), "无窗口时不得置重绘标志");
}

#[test]
fn redraw_without_window_is_dropped() {
    let mut state = EngineState::new();
    assert_eq!(step(&mut state, EngineEvent::Redraw), Vec::new());
    assert!(!state.needs_draw());
}

#[test]
fn message_with_window_requests_redraw() {
    let mut state = created();
    assert_eq!(step(&mut state, EngineEvent::Message), Vec::new());
    assert!(state.needs_draw());
    assert!(state.take_draw_request());
    assert!(!state.needs_draw(), "重绘标志取走后必须清零");
}

#[test]
fn window_destroyed_discards_pending_redraw() {
    let mut state = created();
    step(&mut state, EngineEvent::Message);
    assert!(state.needs_draw());

    assert_eq!(
        step(&mut state, EngineEvent::WindowDestroyed),
        vec![EngineAction::DestroySurface]
    );
    assert!(!state.has_window());
    assert!(
        !state.take_draw_request(),
        "无 surface 后不得再保留重绘请求"
    );
}

// ── 销毁与重建 ──────────────────────────────────────────────────────────────

#[test]
fn window_can_be_destroyed_and_recreated() {
    let mut state = created();
    step(&mut state, EngineEvent::QueueCreated);

    assert_eq!(
        step(&mut state, EngineEvent::WindowDestroyed),
        vec![EngineAction::DestroySurface]
    );
    assert!(!state.has_window());
    // 队列不受窗口销毁影响。
    assert!(state.has_queue());

    let rotated = Viewport {
        width: 1920,
        height: 1080,
        density: 2.0,
    };
    assert_eq!(
        step(&mut state, EngineEvent::WindowCreated(rotated)),
        vec![EngineAction::CreateSurface]
    );
    assert_eq!(state.viewport(), Some(rotated));
}

#[test]
fn queue_can_be_destroyed_and_recreated() {
    let mut state = created();
    step(&mut state, EngineEvent::QueueCreated);

    assert_eq!(
        step(&mut state, EngineEvent::QueueDestroyed),
        vec![EngineAction::DetachQueue]
    );
    assert!(!state.has_queue());
    assert_eq!(
        step(&mut state, EngineEvent::QueueCreated),
        vec![EngineAction::AttachQueue]
    );
}

#[test]
fn redundant_destroy_events_are_ignored() {
    let mut state = created();
    assert_eq!(
        step(&mut state, EngineEvent::QueueDestroyed),
        Vec::new(),
        "未创建队列时的销毁事件应被忽略"
    );
    step(&mut state, EngineEvent::WindowDestroyed);
    assert_eq!(
        step(&mut state, EngineEvent::WindowDestroyed),
        Vec::new(),
        "重复销毁不应产生重复的 DestroySurface"
    );
}

#[test]
fn duplicate_creation_tears_down_the_old_one_first() {
    // 正常路径不会发生，但状态机不得静默泄漏旧资源。
    let mut state = created();
    assert_eq!(
        step(&mut state, EngineEvent::WindowCreated(VP)),
        vec![EngineAction::DestroySurface, EngineAction::CreateSurface]
    );
}

// ── resize ─────────────────────────────────────────────────────────────────

#[test]
fn resize_without_window_is_dropped() {
    let mut state = EngineState::new();
    assert_eq!(
        step(
            &mut state,
            EngineEvent::WindowResized {
                width: 800,
                height: 600
            }
        ),
        Vec::new()
    );
}

#[test]
fn resize_preserves_density_and_requests_redraw() {
    let mut state = created();
    state.take_draw_request(); // 清掉首帧标志

    assert_eq!(
        step(
            &mut state,
            EngineEvent::WindowResized {
                width: 1920,
                height: 1080
            }
        ),
        vec![EngineAction::ResizeSurface]
    );
    assert_eq!(
        state.viewport(),
        Some(Viewport {
            width: 1920,
            height: 1080,
            // resize 回调不携带 density，必须沿用旧值而非回落到 1.0。
            density: 2.0,
        })
    );
    assert!(state.take_draw_request());
}

// ── Quit ───────────────────────────────────────────────────────────────────

#[test]
fn quit_releases_held_resources_then_exits() {
    let mut state = created();
    step(&mut state, EngineEvent::QueueCreated);

    assert_eq!(
        step(&mut state, EngineEvent::Quit),
        vec![
            EngineAction::DestroySurface,
            EngineAction::DetachQueue,
            EngineAction::Exit
        ]
    );
    assert!(state.is_quitting());
    assert!(!state.has_window());
    assert!(!state.has_queue());
}

#[test]
fn quit_without_resources_only_exits() {
    let mut state = EngineState::new();
    assert_eq!(
        step(&mut state, EngineEvent::Quit),
        vec![EngineAction::Exit]
    );
}

#[test]
fn nothing_happens_after_quit() {
    let mut state = created();
    step(&mut state, EngineEvent::QueueCreated);
    step(&mut state, EngineEvent::Quit);

    for event in [
        EngineEvent::WindowCreated(VP),
        EngineEvent::WindowResized {
            width: 1,
            height: 1,
        },
        EngineEvent::WindowDestroyed,
        EngineEvent::QueueCreated,
        EngineEvent::QueueDestroyed,
        EngineEvent::Message,
        EngineEvent::Redraw,
        EngineEvent::Quit,
    ] {
        assert_eq!(
            step(&mut state, event),
            Vec::new(),
            "Quit 之后 {event:?} 不得产生任何动作"
        );
    }
    assert!(!state.needs_draw());
    assert!(!state.take_draw_request());
}

// ── 初始状态 ────────────────────────────────────────────────────────────────

#[test]
fn initial_state_is_empty() {
    let state = EngineState::new();
    assert_eq!(state.viewport(), None);
    assert!(!state.has_window());
    assert!(!state.has_queue());
    assert!(!state.is_quitting());
    assert!(!state.needs_draw());
}
