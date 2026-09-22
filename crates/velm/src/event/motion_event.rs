//! event/motion_event.rs — 触控事件（纯解码 + FFI 薄壳）。
//!
//! 可测性拆分（SPEC §7.3 / §10.2）：action 解码是**纯函数** `decode_action`，
//! 宿主可单测；`from_ndk` 只是「取值 + 调纯函数」的 FFI 薄壳，仅在
//! android 目标编译，内部不含业务分支。

/// NDK action 码的低 8 位掩码（`AMOTION_EVENT_ACTION_MASK`）。
const ACTION_MASK: u32 = 0xff;
/// `AMOTION_EVENT_ACTION_DOWN`：手指按下。
const ACTION_DOWN: u32 = 0;
/// `AMOTION_EVENT_ACTION_UP`：手指抬起。
const ACTION_UP: u32 = 1;
/// `AMOTION_EVENT_ACTION_MOVE`：按下期间的移动。
const ACTION_MOVE: u32 = 2;
/// `AMOTION_EVENT_ACTION_CANCEL`：手势被系统取消。
const ACTION_CANCEL: u32 = 3;

/// 编译期校验：本模块的 u32 常量必须与 raw-ndk-sys 绑定一致，防止手写常量漂移。
#[cfg(target_os = "android")]
const _: () = {
    assert!(ACTION_MASK == raw_ndk_sys::AMOTION_EVENT_ACTION_MASK);
    assert!(ACTION_DOWN == raw_ndk_sys::AMOTION_EVENT_ACTION_DOWN);
    assert!(ACTION_UP == raw_ndk_sys::AMOTION_EVENT_ACTION_UP);
    assert!(ACTION_MOVE == raw_ndk_sys::AMOTION_EVENT_ACTION_MOVE);
    assert!(ACTION_CANCEL == raw_ndk_sys::AMOTION_EVENT_ACTION_CANCEL);
};

/// v1 支持的触控动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchAction {
    /// `AMOTION_EVENT_ACTION_DOWN`。
    ActionDown,
    /// `AMOTION_EVENT_ACTION_MOVE`。
    ActionMove,
    /// `AMOTION_EVENT_ACTION_UP`。
    ActionUp,
    /// `AMOTION_EVENT_ACTION_CANCEL`。
    ActionCancel,
}

/// 把 NDK 原始 action 码映射为 [`TouchAction`]。
///
/// 先与 [`ACTION_MASK`] 求交再匹配，因此对已掩码 / 未掩码的输入结果一致
/// （幂等）。v1 只支持单点：多点触控的 `POINTER_DOWN/POINTER_UP`、
/// `HOVER_*`、`SCROLL`、`BUTTON_*` 等一律返回 `None`，由引擎侧 finishEvent
/// 后丢弃（SPEC §7.3）。
pub fn decode_action(raw_action: u32) -> Option<TouchAction> {
    match raw_action & ACTION_MASK {
        ACTION_DOWN => Some(TouchAction::ActionDown),
        ACTION_UP => Some(TouchAction::ActionUp),
        ACTION_MOVE => Some(TouchAction::ActionMove),
        ACTION_CANCEL => Some(TouchAction::ActionCancel),
        _ => None,
    }
}

/// 一次触控事件（v1 只读取 pointer index 0，坐标为物理像素）。
#[derive(Clone, Debug)]
pub struct MotionEvent {
    /// 解码后的动作。
    pub action: TouchAction,
    /// 触点 x（物理像素，ADR-12）。
    pub x: f32,
    /// 触点 y（物理像素，ADR-12）。
    pub y: f32,
}

impl MotionEvent {
    /// 动作（对齐 Android `get_action` 语义）。
    pub fn action(&self) -> TouchAction {
        self.action
    }

    /// 触点 x（物理像素）。
    pub fn x(&self) -> f32 {
        self.x
    }

    /// 触点 y（物理像素）。
    pub fn y(&self) -> f32 {
        self.y
    }
}

/// FFI 薄壳：只负责取值并调用 [`decode_action`]，不做任何业务分支。
#[cfg(target_os = "android")]
impl MotionEvent {
    /// 从 NDK `AInputEvent` 构造触控事件；非 motion 事件或 v1 不支持的 action 返回 `None`。
    ///
    /// # Safety
    /// `event` 必须为 null 或指向有效的 `AInputEvent`，且在本函数调用期间
    /// 保持有效（引擎线程在 `AInputQueue_getEvent` 之后、`finishEvent` 之前调用）。
    pub unsafe fn from_ndk(event: *const raw_ndk_sys::AInputEvent) -> Option<Self> {
        // SAFETY: 由调用方保证指针有效性；NDK 函数本身不移交所有权。
        unsafe {
            if event.is_null() {
                return None;
            }
            if raw_ndk_sys::AInputEvent_getType(event)
                != raw_ndk_sys::AINPUT_EVENT_TYPE_MOTION as i32
            {
                return None;
            }
            let action = decode_action(raw_ndk_sys::AMotionEvent_getAction(event) as u32)?;
            Some(Self {
                action,
                x: raw_ndk_sys::AMotionEvent_getX(event, 0),
                y: raw_ndk_sys::AMotionEvent_getY(event, 0),
            })
        }
    }
}
