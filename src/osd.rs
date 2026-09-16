//! Toast-style feedback shown when a remap fires.
//!
//! A layered, click-through, non-activating window: it never takes focus and
//! never intercepts clicks. Rounded corners come from a window region, and the
//! whole lifetime - the hold and the fade - is driven by a single repeating
//! `WM_TIMER`, so a banner cannot be left on screen: every tick either keeps it,
//! dims it, or hides it.

use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    FillRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE,
    DT_VCENTER, FF_DONTCARE, FW_NORMAL, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, KillTimer, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW,
    HWND_TOPMOST, LWA_ALPHA, SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Repeating tick that drives both the hold and the fade. One timer for the
/// whole lifetime of a banner: there is no state in which the window is on
/// screen without a tick pending.
const TIMER_TICK: usize = 0x4D01;
/// Resolution of the countdown; [`FADE_MS`] is a multiple of it.
const TICK_MS: u32 = 40;
/// How long the banner takes to fade from fully opaque to invisible.
const FADE_MS: u32 = 1_000;

fn background() -> COLORREF {
    COLORREF(0x0026_2626)
}

fn foreground() -> COLORREF {
    COLORREF(0x00F5_F5F5)
}

struct Osd {
    window: HWND,
    font: windows::Win32::Graphics::Gdi::HFONT,
    brush: windows::Win32::Graphics::Gdi::HBRUSH,
    text: String,
    alpha: u8,
    /// Milliseconds since the current banner appeared.
    elapsed_ms: u32,
    /// Hold time of the current banner, before the fade starts.
    hold_ms: u32,
    visible: bool,
}

// Safety: the OSD window and its GDI objects are only ever touched from the
// shell thread that created them. The global below exists so `show` can be
// called without threading a handle through the window procedure; it is never
// accessed concurrently.
unsafe impl Send for Osd {}

static OSD: OnceLock<Mutex<Option<Osd>>> = OnceLock::new();

fn state() -> &'static Mutex<Option<Osd>> {
    OSD.get_or_init(|| Mutex::new(None))
}

/// Show `text` as a banner for `duration_ms`, then fade it out over
/// [`FADE_MS`] and hide it. Must be called from the thread that owns the tray
/// window, because the OSD window and its GDI objects live on that thread.
pub fn show(text: &str, duration_ms: u32) {
    let mut guard = match state().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if guard.is_none() {
        match create() {
            Some(osd) => *guard = Some(osd),
            None => return,
        }
    }

    let Some(osd) = guard.as_mut() else {
        return;
    };

    let scale = dpi_scale();
    let units: f32 = text
        .chars()
        .map(|character| if character.is_ascii() { 1.0 } else { 2.0 })
        .sum();
    let width = ((units * 8.5 + 56.0) * scale).clamp(220.0 * scale, 620.0 * scale) as i32;
    let height = (52.0 * scale) as i32;

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
    let y = work_area.bottom - height - (28.0 * scale) as i32;

    osd.text = text.to_string();
    osd.alpha = 255;
    osd.elapsed_ms = 0;
    osd.hold_ms = duration_ms.max(TICK_MS);
    osd.visible = true;

    unsafe {
        let _ = SetWindowPos(
            osd.window,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        );

        // Region is owned by the window system once set - do not delete it.
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, (14.0 * scale) as i32, (14.0 * scale) as i32);
        SetWindowRgn(osd.window, Some(region), true);

        let _ = SetLayeredWindowAttributes(osd.window, COLORREF(0), 255, LWA_ALPHA);
        let _ = ShowWindow(osd.window, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(Some(osd.window), None, true);
        // Re-arming the tick restarts the hold, and a banner that was already
        // fading turns opaque again.
        SetTimer(Some(osd.window), TIMER_TICK, TICK_MS, None);
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

fn create() -> Option<Osd> {
    unsafe {
        let class_name = crate::win::wide("MP14Tools.Osd");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: crate::win::module_instance(),
            lpszClassName: crate::win::pcw(&class_name),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            crate::log::line("osd: RegisterClassW failed");
            return None;
        }

        let window = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            crate::win::pcw(&class_name),
            crate::win::pcw(&class_name),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(crate::win::module_instance()),
            None,
        ) {
            Ok(window) => window,
            Err(error) => {
                crate::log::line(&format!("osd: CreateWindowExW failed: {error}"));
                return None;
            }
        };

        let face = crate::win::wide("Microsoft YaHei UI");
        let font = CreateFontW(
            -((16.0 * dpi_scale()) as i32),
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
            (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
            PCWSTR(face.as_ptr()),
        );

        Some(Osd {
            window,
            font,
            brush: CreateSolidBrush(background()),
            text: String::new(),
            alpha: 255,
            elapsed_ms: 0,
            hold_ms: crate::config::OSD_HOLD_DEFAULT_MS,
            visible: false,
        })
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint(window);
            LRESULT(0)
        }
        // We paint the whole client area ourselves.
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            on_timer(wparam.0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn paint(window: HWND) {
    let mut paint_struct = Default::default();
    let device = BeginPaint(window, &mut paint_struct);

    let mut client = RECT::default();
    let _ = GetClientRect(window, &mut client);

    if let Ok(guard) = state().lock() {
        if let Some(osd) = guard.as_ref() {
            FillRect(device, &client, osd.brush);
            SetBkMode(device, TRANSPARENT);
            SetTextColor(device, foreground());

            let previous = SelectObject(device, HGDIOBJ(osd.font.0));
            let mut text: Vec<u16> = osd.text.encode_utf16().collect();
            let mut bounds = client;
            DrawTextW(
                device,
                &mut text,
                &mut bounds,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(device, previous);
        }
    }

    let _ = EndPaint(window, &paint_struct);
}

unsafe fn on_timer(identifier: usize) {
    if identifier != TIMER_TICK {
        return;
    }

    let Ok(mut guard) = state().lock() else {
        return;
    };
    let Some(osd) = guard.as_mut() else {
        return;
    };

    // A banner that is no longer visible must not keep the timer alive.
    if !osd.visible {
        let _ = KillTimer(Some(osd.window), TIMER_TICK);
        return;
    }

    osd.elapsed_ms = osd.elapsed_ms.saturating_add(TICK_MS);
    if osd.elapsed_ms < osd.hold_ms {
        return;
    }

    // Past the hold time the remaining milliseconds map straight onto alpha, so
    // the fade takes exactly FADE_MS from opaque to invisible.
    let faded = osd.elapsed_ms - osd.hold_ms;
    let alpha = if faded >= FADE_MS {
        0
    } else {
        ((FADE_MS - faded) * u32::from(u8::MAX) / FADE_MS) as u8
    };

    if alpha == 0 {
        let _ = KillTimer(Some(osd.window), TIMER_TICK);
        let _ = SetLayeredWindowAttributes(osd.window, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(osd.window, SW_HIDE);
        osd.alpha = 0;
        osd.visible = false;
    } else {
        osd.alpha = alpha;
        let _ = SetLayeredWindowAttributes(osd.window, COLORREF(0), alpha, LWA_ALPHA);
    }
}

/// Release the GDI objects. Called when the owning thread shuts down.
pub fn dispose() {
    if let Ok(mut guard) = state().lock() {
        if let Some(osd) = guard.take() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(osd.font.0));
                let _ = DeleteObject(HGDIOBJ(osd.brush.0));
            }
        }
    }
}
