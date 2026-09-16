//! The application icon, defined once.
//!
//! A rounded square in the product's light blue with a dot in the middle. Three
//! consumers need it in three different forms, so the drawing lives here and
//! nothing re-implements it:
//!
//! * the tray, which needs an `HICON`;
//! * the settings window, whose icon Windows also uses for the taskbar, and
//!   which therefore needs RGBA pixels;
//! * the executable itself, which needs a real `.ico` resource - that one is
//!   generated from these same numbers by `tools/make-icon.ps1` and embedded by
//!   `build.rs`.
//!
//! Keeping the numbers in one place is the whole point: change [`BASE`] and all
//! three follow.

use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    HGDIOBJ, RGBQUAD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, LoadIconW, HICON, ICONINFO, IDI_APPLICATION,
};

/// Base colour of the icon: the product's light blue (`#b4c6da`).
pub const BASE: (u8, u8, u8) = (0xB4, 0xC6, 0xDA);
/// The centre dot is the base mixed towards this near-white.
pub const DOT: (u8, u8, u8) = (0xF8, 0xFB, 0xFF);

/// Corner radius as a fraction of the side.
pub const CORNER: f32 = 0.24;
/// Radius of the centre dot, as a fraction of the side.
pub const DOT_RADIUS: f32 = 0.14;

/// Samples per axis. The tray icon is drawn at 32 px and scaled down by the
/// shell to 16, so the corners have to be anti-aliased rather than aliased.
const SAMPLES: u32 = 4;

/// Side of the icon handed to the tray.
pub const TRAY_SIZE: i32 = 32;

/// Whether a point lies inside the rounded square.
pub fn in_rounded_square(x: f32, y: f32, size: f32, radius: f32) -> bool {
    let nearest_x = x.clamp(radius, size - radius);
    let nearest_y = y.clamp(radius, size - radius);
    let dx = x - nearest_x;
    let dy = y - nearest_y;
    dx * dx + dy * dy <= radius * radius
}

/// RGBA pixels of the icon, row-major from the top.
///
/// This is the form `egui` wants for a window icon, which is what Windows then
/// shows in the taskbar and in Alt-Tab.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let side = size as f32;
    let radius = side * CORNER;
    let centre = side / 2.0;
    let dot = side * DOT_RADIUS;
    let total = (SAMPLES * SAMPLES) as f32;

    for y in 0..size {
        for x in 0..size {
            let mut inside = 0u32;
            let mut dot_hits = 0u32;

            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32;

                    if in_rounded_square(px, py, side, radius) {
                        inside += 1;
                        let dx = px - centre;
                        let dy = py - centre;
                        if dx * dx + dy * dy <= dot * dot {
                            dot_hits += 1;
                        }
                    }
                }
            }

            let coverage = inside as f32 / total;
            let mix = if inside == 0 {
                0.0
            } else {
                dot_hits as f32 / inside as f32
            };

            let index = ((y * size + x) * 4) as usize;
            pixels[index] = blend(BASE.0, DOT.0, mix);
            pixels[index + 1] = blend(BASE.1, DOT.1, mix);
            pixels[index + 2] = blend(BASE.2, DOT.2, mix);
            pixels[index + 3] = (coverage * 255.0).round() as u8;
        }
    }

    pixels
}

/// The icon as an `HICON`, for the tray.
///
/// The bitmap is 32 bpp with a real alpha channel; the mask is left empty
/// because Windows only falls back to it when the alpha channel is missing.
pub fn hicon() -> HICON {
    const SIZE: i32 = TRAY_SIZE;

    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: SIZE,
        // Negative height keeps the bitmap top-down, matching our row order.
        biHeight: -SIZE,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        bmiColors: [RGBQUAD::default()],
    };

    unsafe {
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let colour = match CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) if !bits.is_null() => bitmap,
            _ => return LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        };

        let pixels = rgba(SIZE as u32);
        let target = std::slice::from_raw_parts_mut(bits as *mut u8, pixels.len());
        // The DIB wants BGRA, the RGBA helper produces RGBA.
        for (source, destination) in pixels.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
            destination[0] = source[2];
            destination[1] = source[1];
            destination[2] = source[0];
            destination[3] = source[3];
        }

        let mask = CreateBitmap(SIZE, SIZE, 1, 1, None);
        let descriptor = ICONINFO {
            fIcon: BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: colour,
        };

        let icon = CreateIconIndirect(&descriptor).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(colour.0));
        icon
    }
}

fn blend(base: u8, dot: u8, mix: f32) -> u8 {
    (base as f32 + (dot as f32 - base as f32) * mix).round() as u8
}
