//! Catalogue of everything an [`crate::config::Action`] may target.
//!
//! Built once, lazily, into a `Vec` so the table stays readable instead of
//! spelling out ninety static literals.

use std::sync::OnceLock;

/// A keyboard key that can be used as an action target.
pub struct KeyOption {
    pub id: &'static str,
    pub label: &'static str,
    pub virtual_key: u16,
    /// Set `KEYEVENTF_EXTENDEDKEY` - required by arrows, media and browser keys.
    pub extended: bool,
    /// Grouping used by the settings UI.
    pub group: &'static str,
}

/// A mouse button that can be used as an action target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// Modifier ids accepted in `Action::modifiers`, with their virtual keys.
pub const MODIFIERS: &[(&str, &str, u16)] = &[
    ("ctrl", "Ctrl", 0x11),
    ("shift", "Shift", 0x10),
    ("alt", "Alt", 0x12),
    ("win", "Win", 0x5B),
];

/// Resolve a modifier id (case-insensitive) to its virtual key.
pub fn modifier_virtual_key(id: &str) -> Option<u16> {
    MODIFIERS
        .iter()
        .find(|(modifier_id, _, _)| modifier_id.eq_ignore_ascii_case(id))
        .map(|(_, _, virtual_key)| *virtual_key)
}

/// Group order used by the settings UI.
pub const GROUPS: &[&str] = &[
    "鼠标",
    "字母",
    "数字",
    "功能键",
    "编辑",
    "导航",
    "媒体",
    "浏览器",
    "系统",
    "符号",
];

fn keys() -> &'static Vec<KeyOption> {
    static KEYS: OnceLock<Vec<KeyOption>> = OnceLock::new();
    KEYS.get_or_init(build)
}

fn build() -> Vec<KeyOption> {
    let mut keys = Vec::with_capacity(128);

    for (index, letter) in (b'A'..=b'Z').enumerate() {
        let label: &'static str = LETTERS[index];
        keys.push(KeyOption {
            id: LETTER_IDS[index],
            label,
            virtual_key: letter as u16,
            extended: false,
            group: "字母",
        });
    }

    for (index, digit) in (b'0'..=b'9').enumerate() {
        keys.push(KeyOption {
            id: DIGIT_IDS[index],
            label: DIGIT_LABELS[index],
            virtual_key: digit as u16,
            extended: false,
            group: "数字",
        });
    }

    keys.extend([
        KeyOption { id: "F1", label: "F1", virtual_key: 0x70, extended: false, group: "功能键" },
        KeyOption { id: "F2", label: "F2", virtual_key: 0x71, extended: false, group: "功能键" },
        KeyOption { id: "F3", label: "F3", virtual_key: 0x72, extended: false, group: "功能键" },
        KeyOption { id: "F4", label: "F4", virtual_key: 0x73, extended: false, group: "功能键" },
        KeyOption { id: "F5", label: "F5", virtual_key: 0x74, extended: false, group: "功能键" },
        KeyOption { id: "F6", label: "F6", virtual_key: 0x75, extended: false, group: "功能键" },
        KeyOption { id: "F7", label: "F7", virtual_key: 0x76, extended: false, group: "功能键" },
        KeyOption { id: "F8", label: "F8", virtual_key: 0x77, extended: false, group: "功能键" },
        KeyOption { id: "F9", label: "F9", virtual_key: 0x78, extended: false, group: "功能键" },
        KeyOption { id: "F10", label: "F10", virtual_key: 0x79, extended: false, group: "功能键" },
        KeyOption { id: "F11", label: "F11", virtual_key: 0x7A, extended: false, group: "功能键" },
        KeyOption { id: "F12", label: "F12", virtual_key: 0x7B, extended: false, group: "功能键" },
        KeyOption { id: "F13", label: "F13", virtual_key: 0x7C, extended: false, group: "功能键" },
        KeyOption { id: "F14", label: "F14", virtual_key: 0x7D, extended: false, group: "功能键" },
        KeyOption { id: "F15", label: "F15", virtual_key: 0x7E, extended: false, group: "功能键" },
        KeyOption { id: "F16", label: "F16", virtual_key: 0x7F, extended: false, group: "功能键" },
        KeyOption { id: "F17", label: "F17", virtual_key: 0x80, extended: false, group: "功能键" },
        KeyOption { id: "F18", label: "F18", virtual_key: 0x81, extended: false, group: "功能键" },
        KeyOption { id: "F19", label: "F19", virtual_key: 0x82, extended: false, group: "功能键" },
        KeyOption { id: "F20", label: "F20", virtual_key: 0x83, extended: false, group: "功能键" },
        KeyOption { id: "F21", label: "F21", virtual_key: 0x84, extended: false, group: "功能键" },
        KeyOption { id: "F22", label: "F22", virtual_key: 0x85, extended: false, group: "功能键" },
        KeyOption { id: "F23", label: "F23", virtual_key: 0x86, extended: false, group: "功能键" },
        KeyOption { id: "F24", label: "F24", virtual_key: 0x87, extended: false, group: "功能键" },
        KeyOption { id: "Enter", label: "Enter", virtual_key: 0x0D, extended: false, group: "编辑" },
        KeyOption { id: "Space", label: "Space", virtual_key: 0x20, extended: false, group: "编辑" },
        KeyOption { id: "Tab", label: "Tab", virtual_key: 0x09, extended: false, group: "编辑" },
        KeyOption { id: "Backspace", label: "Backspace", virtual_key: 0x08, extended: false, group: "编辑" },
        KeyOption { id: "Escape", label: "Esc", virtual_key: 0x1B, extended: false, group: "编辑" },
        KeyOption { id: "Delete", label: "Delete", virtual_key: 0x2E, extended: true, group: "编辑" },
        KeyOption { id: "Insert", label: "Insert", virtual_key: 0x2D, extended: true, group: "编辑" },
        KeyOption { id: "ArrowUp", label: "↑", virtual_key: 0x26, extended: true, group: "导航" },
        KeyOption { id: "ArrowDown", label: "↓", virtual_key: 0x28, extended: true, group: "导航" },
        KeyOption { id: "ArrowLeft", label: "←", virtual_key: 0x25, extended: true, group: "导航" },
        KeyOption { id: "ArrowRight", label: "→", virtual_key: 0x27, extended: true, group: "导航" },
        KeyOption { id: "Home", label: "Home", virtual_key: 0x24, extended: true, group: "导航" },
        KeyOption { id: "End", label: "End", virtual_key: 0x23, extended: true, group: "导航" },
        KeyOption { id: "PageUp", label: "Page Up", virtual_key: 0x21, extended: true, group: "导航" },
        KeyOption { id: "PageDown", label: "Page Down", virtual_key: 0x22, extended: true, group: "导航" },
        KeyOption { id: "VolumeMute", label: "静音", virtual_key: 0xAD, extended: true, group: "媒体" },
        KeyOption { id: "VolumeDown", label: "音量 -", virtual_key: 0xAE, extended: true, group: "媒体" },
        KeyOption { id: "VolumeUp", label: "音量 +", virtual_key: 0xAF, extended: true, group: "媒体" },
        KeyOption { id: "MediaNext", label: "下一曲", virtual_key: 0xB0, extended: true, group: "媒体" },
        KeyOption { id: "MediaPrev", label: "上一曲", virtual_key: 0xB1, extended: true, group: "媒体" },
        KeyOption { id: "MediaStop", label: "停止", virtual_key: 0xB2, extended: true, group: "媒体" },
        KeyOption { id: "MediaPlayPause", label: "播放/暂停", virtual_key: 0xB3, extended: true, group: "媒体" },
        KeyOption { id: "BrowserBack", label: "后退", virtual_key: 0xA6, extended: true, group: "浏览器" },
        KeyOption { id: "BrowserForward", label: "前进", virtual_key: 0xA7, extended: true, group: "浏览器" },
        KeyOption { id: "BrowserRefresh", label: "刷新", virtual_key: 0xA8, extended: true, group: "浏览器" },
        KeyOption { id: "BrowserHome", label: "主页", virtual_key: 0xAC, extended: true, group: "浏览器" },
        KeyOption { id: "CapsLock", label: "Caps Lock", virtual_key: 0x14, extended: false, group: "系统" },
        KeyOption { id: "NumLock", label: "Num Lock", virtual_key: 0x90, extended: true, group: "系统" },
        KeyOption { id: "ScrollLock", label: "Scroll Lock", virtual_key: 0x91, extended: false, group: "系统" },
        KeyOption { id: "PrintScreen", label: "Print Screen", virtual_key: 0x2C, extended: true, group: "系统" },
        KeyOption { id: "Pause", label: "Pause", virtual_key: 0x13, extended: false, group: "系统" },
        KeyOption { id: "MetaLeft", label: "Win", virtual_key: 0x5B, extended: true, group: "系统" },
        KeyOption { id: "ContextMenu", label: "菜单键", virtual_key: 0x5D, extended: true, group: "系统" },
        KeyOption { id: "Backquote", label: "`", virtual_key: 0xC0, extended: false, group: "符号" },
        KeyOption { id: "Minus", label: "-", virtual_key: 0xBD, extended: false, group: "符号" },
        KeyOption { id: "Equal", label: "=", virtual_key: 0xBB, extended: false, group: "符号" },
        KeyOption { id: "BracketLeft", label: "[", virtual_key: 0xDB, extended: false, group: "符号" },
        KeyOption { id: "BracketRight", label: "]", virtual_key: 0xDD, extended: false, group: "符号" },
        KeyOption { id: "Backslash", label: "\\", virtual_key: 0xDC, extended: false, group: "符号" },
        KeyOption { id: "Semicolon", label: ";", virtual_key: 0xBA, extended: false, group: "符号" },
        KeyOption { id: "Quote", label: "'", virtual_key: 0xDE, extended: false, group: "符号" },
        KeyOption { id: "Comma", label: ",", virtual_key: 0xBC, extended: false, group: "符号" },
        KeyOption { id: "Period", label: ".", virtual_key: 0xBE, extended: false, group: "符号" },
        KeyOption { id: "Slash", label: "/", virtual_key: 0xBF, extended: false, group: "符号" },
    ]);

    keys.sort_by_key(|option| {
        GROUPS
            .iter()
            .position(|group| *group == option.group)
            .unwrap_or(usize::MAX)
    });

    keys
}

const LETTER_IDS: [&str; 26] = [
    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL",
    "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX",
    "KeyY", "KeyZ",
];

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

const DIGIT_IDS: [&str; 10] = [
    "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8",
    "Digit9",
];

const DIGIT_LABELS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// All keyboard targets, grouped in `GROUPS` order.
pub fn key_options() -> &'static [KeyOption] {
    keys()
}

/// All mouse targets, kept out of [`key_options`] because they need different
/// `SendInput` structures.
pub const MOUSE_IDS: &[(&str, &str)] = &[
    ("MouseLeft", "鼠标左键"),
    ("MouseMiddle", "鼠标中键"),
    ("MouseRight", "鼠标右键"),
    ("MouseX1", "鼠标侧键 1"),
    ("MouseX2", "鼠标侧键 2"),
];

/// Look up a keyboard target by id (case-insensitive).
pub fn find_key(id: &str) -> Option<&'static KeyOption> {
    keys()
        .iter()
        .find(|option| option.id.eq_ignore_ascii_case(id))
}

/// Look up a mouse target by id (case-insensitive).
pub fn find_mouse(id: &str) -> Option<MouseButton> {
    match id.to_ascii_lowercase().as_str() {
        "mouseleft" => Some(MouseButton::Left),
        "mouseright" => Some(MouseButton::Right),
        "mousemiddle" => Some(MouseButton::Middle),
        "mousex1" => Some(MouseButton::X1),
        "mousex2" => Some(MouseButton::X2),
        _ => None,
    }
}

/// Human-readable label for any target id, used by the UI and the OSD.
pub fn label_of(id: &str) -> String {
    if let Some(option) = find_key(id) {
        return option.label.to_string();
    }
    if let Some((_, label)) = MOUSE_IDS
        .iter()
        .find(|(mouse_id, _)| mouse_id.eq_ignore_ascii_case(id))
    {
        return (*label).to_string();
    }
    id.to_string()
}
