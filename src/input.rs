//! Turns an [`Action`] into real input via `SendInput`.
//!
//! Only keyboard keys and mouse buttons are supported, which is exactly what the
//! lite build promises: an OEM key or a touchpad deep press becomes "some key,
//! mouse button, or any combination of them".

use std::mem::size_of;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOUSEINPUT, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, VIRTUAL_KEY,
};

use crate::catalog::{self, MouseButton};
use crate::config::Action;

/// `mouseData` values identifying the two extra mouse buttons.
const XBUTTON1_DATA: u32 = 0x0001;
const XBUTTON2_DATA: u32 = 0x0002;

/// Send `action`. Returns a description of what was sent, or a reason why not.
pub fn send(action: &Action) -> Result<String, String> {
    let Some(target) = action.target.as_deref() else {
        return Err("未设置目标按键".to_string());
    };

    let modifiers: Vec<u16> = action
        .modifiers
        .iter()
        .filter_map(|modifier| catalog::modifier_virtual_key(modifier))
        .collect();

    let mut inputs: Vec<INPUT> = Vec::with_capacity(modifiers.len() * 2 + 2);

    // Modifiers down (in declared order).
    for virtual_key in &modifiers {
        inputs.push(key_input(*virtual_key, false, false));
    }

    if let Some(button) = catalog::find_mouse(target) {
        inputs.push(mouse_input(button, true));
        inputs.push(mouse_input(button, false));
    } else if let Some(option) = catalog::find_key(target) {
        inputs.push(key_input(option.virtual_key, option.extended, false));
        inputs.push(key_input(option.virtual_key, option.extended, true));
    } else {
        return Err(format!("未知的目标按键: {target}"));
    }

    // Modifiers up, reversed so the chord unwinds in order.
    for virtual_key in modifiers.iter().rev() {
        inputs.push(key_input(*virtual_key, false, true));
    }

    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("SendInput 被系统拒绝（可能被权限更高的窗口拦截）".to_string());
    }

    Ok(describe(action))
}

/// Human-readable summary such as `Ctrl + Shift + 鼠标中键`.
pub fn describe(action: &Action) -> String {
    let mut parts: Vec<String> = action
        .modifiers
        .iter()
        .filter_map(|modifier| {
            catalog::MODIFIERS
                .iter()
                .find(|(id, _, _)| id.eq_ignore_ascii_case(modifier))
                .map(|(_, label, _)| (*label).to_string())
        })
        .collect();

    match action.target.as_deref() {
        Some(target) => parts.push(catalog::label_of(target)),
        None => return "未设置".to_string(),
    }

    parts.join(" + ")
}

fn key_input(virtual_key: u16, extended: bool, key_up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(button: MouseButton, down: bool) -> INPUT {
    let (flags, data) = match (button, down) {
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1_DATA),
        (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1_DATA),
        (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2_DATA),
        (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2_DATA),
    };

    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
