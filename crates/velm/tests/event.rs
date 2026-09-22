//! event 模块契约测试（SPEC §7.3、§10.1）。
//!
//! 覆盖：4 种 action 解码、带 pointer index 位的掩码值、v1 不支持的 action 与
//! 非法值返回 None、掩码幂等、MotionEvent getter。

use velm::event::{MotionEvent, TouchAction, decode_action};

/// NDK `AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT = 8`：pointer index 位于高 8 位。
const POINTER_INDEX_SHIFT: u32 = 8;

/// 构造「action | (pointer_index << 8)」形式的原始 action 码。
fn with_pointer_index(action: u32, pointer_index: u32) -> u32 {
    action | (pointer_index << POINTER_INDEX_SHIFT)
}

#[test]
fn decode_action_maps_four_supported_actions() {
    assert_eq!(decode_action(0), Some(TouchAction::ActionDown));
    assert_eq!(decode_action(1), Some(TouchAction::ActionUp));
    assert_eq!(decode_action(2), Some(TouchAction::ActionMove));
    assert_eq!(decode_action(3), Some(TouchAction::ActionCancel));
}

#[test]
fn decode_action_strips_pointer_index_bits() {
    // DOWN 恒定伴随 pointer index 0，但高 8 位带 index 时仍须解码为 DOWN。
    assert_eq!(
        decode_action(with_pointer_index(0, 1)),
        Some(TouchAction::ActionDown)
    );
    assert_eq!(
        decode_action(with_pointer_index(0, 3)),
        Some(TouchAction::ActionDown)
    );
    // MOVE / UP / CANCEL 同理。
    assert_eq!(
        decode_action(with_pointer_index(2, 1)),
        Some(TouchAction::ActionMove)
    );
    assert_eq!(
        decode_action(with_pointer_index(1, 2)),
        Some(TouchAction::ActionUp)
    );
    assert_eq!(
        decode_action(with_pointer_index(3, 1)),
        Some(TouchAction::ActionCancel)
    );
}

#[test]
fn decode_action_ignores_high_bits_only() {
    // 仅高 8 位有值、低 8 位为 0 → DOWN（掩码幂等：对已掩码输入同样成立）。
    assert_eq!(decode_action(0xff00), Some(TouchAction::ActionDown));
    assert_eq!(decode_action(0x0100), Some(TouchAction::ActionDown));
    assert_eq!(decode_action(0), Some(TouchAction::ActionDown));
}

#[test]
fn decode_action_returns_none_for_multi_touch_and_unsupported() {
    // v1 只支持单点：POINTER_DOWN(5) / POINTER_UP(6) 即使带 index 也返回 None。
    assert_eq!(decode_action(5), None, "POINTER_DOWN");
    assert_eq!(decode_action(with_pointer_index(5, 1)), None);
    assert_eq!(decode_action(6), None, "POINTER_UP");
    assert_eq!(decode_action(with_pointer_index(6, 2)), None);

    // 其余 action：OUTSIDE(4) / HOVER_MOVE(7) / SCROLL(8) / HOVER_ENTER(9) /
    // HOVER_EXIT(10) / BUTTON_PRESS(11) / BUTTON_RELEASE(12)。
    for raw in [4u32, 7, 8, 9, 10, 11, 12] {
        assert_eq!(
            decode_action(raw),
            None,
            "action {} must be unsupported",
            raw
        );
    }
}

#[test]
fn decode_action_returns_none_for_illegal_values() {
    // 掩码后落在 0..=3 之外的一切值均不支持。
    for raw in [13u32, 14, 42, 100, 127, 128, 200, 254, 255] {
        assert_eq!(decode_action(raw), None, "raw {} must be illegal", raw);
    }
    assert_eq!(decode_action(u32::MAX), None);
}

#[test]
fn motion_event_getters_expose_action_and_coordinates() {
    let event = MotionEvent {
        action: TouchAction::ActionDown,
        x: 12.5,
        y: 240.0,
    };

    assert_eq!(event.action(), TouchAction::ActionDown);
    assert_eq!(event.x(), 12.5);
    assert_eq!(event.y(), 240.0);
    // 字段同样公开可读（渲染/hit-test 直接取用）。
    assert_eq!(event.action, TouchAction::ActionDown);
    assert_eq!(event.x, 12.5);
    assert_eq!(event.y, 240.0);
}

#[test]
fn motion_event_is_cloneable_without_ndk() {
    let event = MotionEvent {
        action: TouchAction::ActionMove,
        x: 1.0,
        y: 2.0,
    };
    let cloned = event.clone();

    assert_eq!(cloned.action(), event.action());
    assert_eq!(cloned.x(), event.x());
    assert_eq!(cloned.y(), event.y());
}

#[test]
fn touch_action_derives_eq_and_copy() {
    let action = TouchAction::ActionCancel;
    let copied = action;

    assert_eq!(action, copied);
    assert_ne!(TouchAction::ActionDown, TouchAction::ActionUp);
}
