//! A small always-on-top notice with up to two buttons.
//!
//! Used by the display policy: "switch to 60 Hz?" and "turn HDR off?" both need
//! an answer, and a plain toast (see [`crate::osd`]) cannot be clicked. The
//! window is deliberately its own thing rather than a second mode of the OSD:
//! the OSD is click-through by design, and mixing the two would mean giving up
//! that property for every keystroke feedback.
//!
//! Threading follows the same rule as the OSD - the window and its GDI objects
//! live on the shell thread - so the API is split in two:
//!
//! * any thread calls [`ask`], which queues the notice, wakes the shell thread
//!   and blocks until a button is clicked or the timeout runs out;
//! * the shell thread calls [`serve`] when it receives the wake-up message.
//!
//! Only one notice is ever on screen: a newer one replaces a queued one, so a
//! burst of power events cannot leave stale questions behind.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, InvalidateRect, RoundRect, SelectObject, SetBkMode, SetTextColor,
    SetWindowRgn, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, FW_SEMIBOLD, HGDIOBJ,
    OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, KillTimer, RegisterClassW, SetLayeredWindowAttributes,
    SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, HWND_TOPMOST, LWA_ALPHA,
    SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_ERASEBKGND, WM_LBUTTONUP, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// How long a notice waits for an answer before it withdraws itself.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(25);
/// Timer that withdraws the window when nobody answers.
const TIMER_TIMEOUT: usize = 0x4E01;

const PADDING: f32 = 18.0;
const BUTTON_HEIGHT: f32 = 30.0;
const BUTTON_GAP: f32 = 10.0;
const TEXT_LINE: f32 = 21.0;

/// Which button the user picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    First,
    Second,
    /// Timed out, replaced by a newer notice, or the window could not be shown.
    Dismissed,
}

/// One question: a line of text and one or two buttons.
#[derive(Clone, Debug)]
pub struct Notice {
    pub text: String,
    pub first: String,
    pub second: Option<String>,
}

impl Notice {
    pub fn new(text: impl Into<String>, first: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            first: first.into(),
            second: None,
        }
    }

    pub fn with_second(mut self, label: impl Into<String>) -> Self {
        self.second = Some(label.into());
        self
    }
}

/// Requests waiting for the shell thread, newest last.
static QUEUE: Mutex<VecDeque<(Notice, Sender<Choice>)>> = Mutex::new(VecDeque::new());
static WINDOW: OnceLock<Mutex<Option<Panel>>> = OnceLock::new();

fn panel() -> &'static Mutex<Option<Panel>> {
    WINDOW.get_or_init(|| Mutex::new(None))
}

fn queue() -> &'static Mutex<VecDeque<(Notice, Sender<Choice>)>> {
    &QUEUE
}

/// The window, its GDI objects and whatever it is currently asking.
struct Panel {
    window: HWND,
    font_text: windows::Win32::Graphics::Gdi::HFONT,
    font_button: windows::Win32::Graphics::Gdi::HFONT,
    background: windows::Win32::Graphics::Gdi::HBRUSH,
    button: windows::Win32::Graphics::Gdi::HBRUSH,
    notice: Notice,
    reply: Sender<Choice>,
    /// Clickable areas, in client coordinates.
    first_rect: RECT,
    second_rect: Option<RECT>,
    answered: bool,
}

// Safety: only the shell thread ever touches the window or the GDI objects; the
// global exists so the window procedure can reach them.
unsafe impl Send for Panel {}

/// Queue a notice and block until one of its buttons is clicked.
///
/// Called from the display policy thread. Times out rather than waiting forever,
/// so a notice that never gets shown cannot wedge the caller.
pub fn ask(notice: Notice) -> Choice {
    let (sender, receiver) = mpsc::channel();

    {
        let Ok(mut queue) = queue().lock() else {
            return Choice::Dismissed;
        };
        // Superseded questions are not worth answering, and clearing keeps the
        // shell thread from showing a stale one first.
        for (_, stale) in queue.drain(..) {
            let _ = stale.send(Choice::Dismissed);
        }
        queue.push_back((notice, sender));
    }

    crate::tray::wake_shell();
    receiver.recv_timeout(ANSWER_TIMEOUT).unwrap_or(Choice::Dismissed)
}

/// Show the newest queued notice. Must run on the shell thread.
pub fn serve() {
    let Some((notice, reply)) = queue().lock().ok().and_then(|mut queue| queue.pop_back())
    else {
        return;
    };

    let mut guard = match panel().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if guard.is_none() {
        match create() {
            Some(panel) => *guard = Some(panel),
            None => {
                let _ = reply.send(Choice::Dismissed);
                return;
            }
        }
    }

    let Some(panel) = guard.as_mut() else {
        return;
    };

    // A notice that is still on screen is answered by the new one taking over.
    if !panel.answered {
        let _ = panel.reply.send(Choice::Dismissed);
    }
    panel.answered = false;
    panel.notice = notice;
    panel.reply = reply;

    layout(panel);
}

/// Close the notice window; called when the shell thread shuts down.
pub fn dispose() {
    let Ok(mut guard) = panel().lock() else {
        return;
    };
    let Some(panel) = guard.take() else {
        return;
    };

    let _ = panel.reply.send(Choice::Dismissed);
    unsafe {
        let _ = KillTimer(Some(panel.window), TIMER_TIMEOUT);
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(panel.window);
        let _ = DeleteObject(HGDIOBJ(panel.font_text.0));
        let _ = DeleteObject(HGDIOBJ(panel.font_button.0));
        let _ = DeleteObject(HGDIOBJ(panel.background.0));
        let _ = DeleteObject(HGDIOBJ(panel.button.0));
    }
}

fn dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

/// Approximate width of a string, in pixels: CJK glyphs are about twice as wide
/// as Latin ones, which is all the accuracy a one-line notice needs.
fn text_width(text: &str, scale: f32) -> f32 {
    let units: f32 = text
        .chars()
        .map(|character| if character.is_ascii() { 1.0 } else { 2.0 })
        .sum();
    units * 7.6 * scale
}

/// Size and place the window for the notice it currently holds.
fn layout(panel: &mut Panel) {
    let scale = dpi_scale();
    let body_width = text_width(&panel.notice.text, scale).min(560.0 * scale);

    let first_width = text_width(&panel.notice.first, scale) + 34.0 * scale;
    let second_width = panel
        .notice
        .second
        .as_ref()
        .map(|label| text_width(label, scale) + 34.0 * scale)
        .unwrap_or(0.0);

    let buttons_width = first_width + second_width + if second_width > 0.0 { BUTTON_GAP * scale } else { 0.0 };
    let width = (body_width.max(buttons_width) + PADDING * 2.0).max(280.0 * scale) as i32;
    let height = (PADDING * 2.0 + TEXT_LINE * scale + 18.0 * scale + BUTTON_HEIGHT * scale) as i32;

    let mut work_area = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }

    let x = work_area.left + ((work_area.right - work_area.left) - width) / 2;
    let y = work_area.bottom - height - (60.0 * scale) as i32;

    // Buttons sit centred under the text.
    let button_y = (PADDING * scale + TEXT_LINE * scale + 18.0 * scale) as i32;
    let start_x = ((width as f32 - buttons_width) / 2.0) as i32;
    panel.first_rect = RECT {
        left: start_x,
        top: button_y,
        right: start_x + first_width as i32,
        bottom: button_y + (BUTTON_HEIGHT * scale) as i32,
    };
    panel.second_rect = (second_width > 0.0).then(|| RECT {
        left: start_x + first_width as i32 + (BUTTON_GAP * scale) as i32,
        top: button_y,
        right: start_x + first_width as i32 + (BUTTON_GAP * scale) as i32 + second_width as i32,
        bottom: button_y + (BUTTON_HEIGHT * scale) as i32,
    });

    unsafe {
        let _ = SetWindowPos(
            panel.window,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        );

        // Region is owned by the window system once set - do not delete it.
        let region = CreateRoundRectRgn(
            0,
            0,
            width + 1,
            height + 1,
            (14.0 * scale) as i32,
            (14.0 * scale) as i32,
        );
        SetWindowRgn(panel.window, Some(region), true);

        let _ = SetLayeredWindowAttributes(panel.window, COLORREF(0), 255, LWA_ALPHA);
        let _ = ShowWindow(panel.window, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(Some(panel.window), None, true);
        SetTimer(Some(panel.window), TIMER_TIMEOUT, 25_000, None);
    }
}

fn create() -> Option<Panel> {
    unsafe {
        let class_name = crate::win::wide("MP14Tools.Notice");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: crate::win::module_instance(),
            lpszClassName: crate::win::pcw(&class_name),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            crate::log::line("notice: RegisterClassW failed");
            return None;
        }

        // Unlike the OSD this one must receive clicks, so no WS_EX_TRANSPARENT.
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            crate::win::pcw(&class_name),
            crate::win::pcw(&class_name),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(crate::win::module_instance()),
            None,
        )
        .ok()?;

        let scale = dpi_scale();
        let font_text = CreateFontW(
            -(14.0 * scale) as i32,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            windows::core::w!("Segoe UI"),
        );
        let font_button = CreateFontW(
            -(13.0 * scale) as i32,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            windows::core::w!("Segoe UI"),
        );

        let (sender, _receiver) = mpsc::channel();

        Some(Panel {
            window,
            font_text,
            font_button,
            background: CreateSolidBrush(COLORREF(0x0026_2626)),
            button: CreateSolidBrush(COLORREF(0x003A_3A3A)),
            notice: Notice::new("", ""),
            reply: sender,
            first_rect: RECT::default(),
            second_rect: None,
            answered: true,
        })
    }
}

/// Answer the current notice and take it off screen.
fn answer(panel: &mut Panel, choice: Choice) {
    if panel.answered {
        return;
    }
    panel.answered = true;

    unsafe {
        let _ = KillTimer(Some(panel.window), TIMER_TIMEOUT);
        let _ = ShowWindow(panel.window, SW_HIDE);
    }

    let _ = panel.reply.send(choice);
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_LBUTTONUP {
        let x = (lparam.0 & 0xFFFF) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

        if let Ok(mut guard) = panel().lock() {
            if let Some(panel) = guard.as_mut() {
                if inside(panel.first_rect, x, y) {
                    answer(panel, Choice::First);
                } else if panel.second_rect.map(|rect| inside(rect, x, y)) == Some(true) {
                    answer(panel, Choice::Second);
                }
            }
        }
        return LRESULT(0);
    }

    if message == WM_TIMER && wparam.0 == TIMER_TIMEOUT {
        if let Ok(mut guard) = panel().lock() {
            if let Some(panel) = guard.as_mut() {
                answer(panel, Choice::Dismissed);
            }
        }
        return LRESULT(0);
    }

    if message == WM_PAINT {
        paint(window);
        return LRESULT(0);
    }

    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }

    DefWindowProcW(window, message, wparam, lparam)
}

fn inside(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn paint(window: HWND) {
    let Ok(mut guard) = panel().lock() else {
        return;
    };
    let Some(panel) = guard.as_mut() else {
        return;
    };

    let mut paint = Default::default();
    let device = unsafe { BeginPaint(window, &mut paint) };

    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut client);
    }

    unsafe {
        let _ = FillRect(device, &client, panel.background);

        SetBkMode(device, TRANSPARENT);
        SetTextColor(device, COLORREF(0x00F5_F5F5));

        let scale = dpi_scale();
        let old = SelectObject(device, HGDIOBJ(panel.font_text.0));

        let mut text: Vec<u16> = panel.notice.text.encode_utf16().collect();
        let mut text_rect = RECT {
            left: (PADDING * scale) as i32,
            top: (PADDING * scale) as i32,
            right: client.right - (PADDING * scale) as i32,
            bottom: (PADDING * scale) as i32 + (TEXT_LINE * scale) as i32,
        };
        DrawTextW(
            device,
            &mut text,
            &mut text_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        SelectObject(device, old);

        // Buttons last, so they sit on top of the background fill.
        let old = SelectObject(device, HGDIOBJ(panel.font_button.0));
        let draw_button = |rect: RECT, label: &str| {
            let radius = ((rect.bottom - rect.top) / 2) as i32;
            let _ = RoundRect(device, rect.left, rect.top, rect.right, rect.bottom, radius, radius);
            let mut label_rect = rect;
            let mut text: Vec<u16> = label.encode_utf16().collect();
            DrawTextW(
                device,
                &mut text,
                &mut label_rect,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
        };

        SelectObject(device, HGDIOBJ(panel.button.0));
        draw_button(panel.first_rect, &panel.notice.first.clone());
        if let (Some(rect), Some(label)) = (panel.second_rect, panel.notice.second.clone()) {
            draw_button(rect, &label);
        }
        SelectObject(device, old);
    }

    unsafe {
        let _ = EndPaint(window, &paint);
    }
}
