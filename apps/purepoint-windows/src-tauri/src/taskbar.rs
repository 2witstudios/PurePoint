//! Taskbar overlay badge (WIN-7.7, REQ-WINAPP-078): the count of agents that need attention, drawn
//! as a small red disc over the app's taskbar button with `ITaskbarList3::SetOverlayIcon`. It
//! replaces the Mac's dock tile badge. Zero clears the overlay.
//!
//! The badge is drawn here pixel by pixel (a disc and a 3x5 digit font), so it needs no image
//! assets and no GDI text, whose output carries no alpha.

/// The badge's fill (Windows' "critical" red, `#C42B1C`), as straight-alpha BGRA bytes' RGB.
const FILL: (u8, u8, u8) = (0xC4, 0x2B, 0x1C);
/// Supersamples per pixel side for the disc's anti-aliased edge.
const SUPERSAMPLE: u32 = 4;

/// 3x5 glyphs, one row per entry, the high of three bits leftmost.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];
const PLUS: [u8; 5] = [0b000, 0b010, 0b111, 0b010, 0b000];

/// What the badge reads: the count, or `9+` past nine.
pub fn badge_text(count: u32) -> String {
    if count > 9 {
        "9+".to_owned()
    } else {
        count.to_string()
    }
}

/// The accessible description Windows reads for the overlay.
pub fn badge_description(count: u32) -> String {
    if count == 1 {
        "1 agent needs attention".to_owned()
    } else {
        format!("{count} agents need attention")
    }
}

fn glyph(c: char) -> Option<&'static [u8; 5]> {
    match c {
        '+' => Some(&PLUS),
        _ => c.to_digit(10).map(|d| &DIGITS[d as usize]),
    }
}

/// The badge as `size`x`size` straight-alpha BGRA pixels, rows top to bottom (`size` >= 8).
pub fn render_badge(size: u32, count: u32) -> Vec<u8> {
    let size = size.max(8);
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let radius = size as f64 / 2.0;
    let samples = f64::from(SUPERSAMPLE * SUPERSAMPLE);
    for y in 0..size {
        for x in 0..size {
            let mut inside = 0u32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SUPERSAMPLE);
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SUPERSAMPLE);
                    let (dx, dy) = (px - radius, py - radius);
                    if dx * dx + dy * dy <= radius * radius {
                        inside += 1;
                    }
                }
            }
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = FILL.2;
            pixels[i + 1] = FILL.1;
            pixels[i + 2] = FILL.0;
            pixels[i + 3] = (f64::from(inside) / samples * 255.0).round() as u8;
        }
    }
    let text = badge_text(count);
    let glyphs: Vec<&[u8; 5]> = text.chars().filter_map(glyph).collect();
    // One glyph fills ~5/8 of the height; two shrink to fit inside the disc.
    let scale = if glyphs.len() > 1 {
        (size / 11).max(1)
    } else {
        (size / 8).max(1)
    };
    let width = (glyphs.len() as u32 * 4 - 1) * scale;
    let height = 5 * scale;
    let left = (size.saturating_sub(width)) / 2;
    let top = (size.saturating_sub(height)) / 2;
    for (index, rows) in glyphs.iter().enumerate() {
        let origin = left + index as u32 * 4 * scale;
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3u32 {
                if bits & (0b100 >> column) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = origin + column * scale + dx;
                        let y = top + row as u32 * scale + dy;
                        if x < size && y < size {
                            let i = ((y * size + x) * 4) as usize;
                            pixels[i..i + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
                        }
                    }
                }
            }
        }
    }
    pixels
}

/// Show `count` on the main window's taskbar button; 0 clears it.
#[tauri::command]
pub fn taskbar_set_badge(window: tauri::WebviewWindow, count: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        // The taskbar's COM object belongs on the window's (STA) thread.
        window
            .run_on_main_thread(move || {
                // SAFETY: `hwnd` is the live main window and this runs on its thread.
                if let Err(error) = unsafe { win::set_overlay(hwnd, count) } {
                    eprintln!("PurePoint: could not set the taskbar badge: {error}");
                }
            })
            .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, count);
        Ok(())
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::HICON;
    use windows::core::PCWSTR;
    use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, DestroyIcon, GetSystemMetrics, ICONINFO, SM_CXSMICON,
    };

    /// # Safety
    /// `hwnd` must be a live top-level window, and the caller on its thread.
    pub unsafe fn set_overlay(hwnd: isize, count: u32) -> windows::core::Result<()> {
        unsafe {
            let taskbar: ITaskbarList3 =
                CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            taskbar.HrInit()?;
            let window = HWND(hwnd as *mut core::ffi::c_void);
            if count == 0 {
                return taskbar.SetOverlayIcon(window, HICON::default(), PCWSTR::null());
            }
            let size = u32::try_from(GetSystemMetrics(SM_CXSMICON))
                .unwrap_or(16)
                .max(16);
            let pixels = super::render_badge(size, count);
            let side = size as i32;
            let color = CreateBitmap(side, side, 1, 32, pixels.as_ptr().cast());
            let mask = CreateBitmap(side, side, 1, 1, std::ptr::null());
            let info = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            };
            let icon = CreateIconIndirect(&info);
            DeleteObject(color);
            DeleteObject(mask);
            if icon.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            let description: Vec<u16> = super::badge_description(count)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            // The taskbar keeps its own copy of the icon.
            let result = taskbar.SetOverlayIcon(window, HICON(icon), PCWSTR(description.as_ptr()));
            DestroyIcon(icon);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(pixels: &[u8], size: u32, x: u32, y: u32) -> u8 {
        pixels[((y * size + x) * 4 + 3) as usize]
    }

    fn is_white(pixels: &[u8], size: u32, x: u32, y: u32) -> bool {
        let i = ((y * size + x) * 4) as usize;
        pixels[i..i + 4] == [0xFF, 0xFF, 0xFF, 0xFF]
    }

    #[test]
    fn caps_the_text_at_nine_plus() {
        assert_eq!(badge_text(1), "1");
        assert_eq!(badge_text(9), "9");
        assert_eq!(badge_text(10), "9+");
        assert_eq!(badge_text(250), "9+");
    }

    #[test]
    fn describes_the_count() {
        assert_eq!(badge_description(1), "1 agent needs attention");
        assert_eq!(badge_description(3), "3 agents need attention");
    }

    #[test]
    fn draws_an_opaque_disc_with_transparent_corners() {
        let pixels = render_badge(16, 3);
        assert_eq!(pixels.len(), 16 * 16 * 4);
        assert_eq!(alpha(&pixels, 16, 0, 0), 0);
        assert_eq!(alpha(&pixels, 16, 15, 15), 0);
        assert_eq!(alpha(&pixels, 16, 8, 1), 255);
        assert_eq!(alpha(&pixels, 16, 1, 8), 255);
    }

    #[test]
    fn draws_the_digit_in_white() {
        // "1" at scale 2 is 6x10 at (5, 3): its stem is the middle column.
        let pixels = render_badge(16, 1);
        assert!(is_white(&pixels, 16, 7, 4));
        assert!(!is_white(&pixels, 16, 5, 4));
        let whites = (0..16 * 16)
            .filter(|i| is_white(&pixels, 16, i % 16, i / 16))
            .count();
        // The glyph's 8 lit cells at 2x2 each.
        assert_eq!(whites, 8 * 4);
    }

    #[test]
    fn fits_nine_plus_and_larger_sizes() {
        for size in [16, 20, 24, 32] {
            let pixels = render_badge(size, 42);
            assert_eq!(pixels.len(), (size * size * 4) as usize);
            assert!((0..size * size).any(|i| is_white(&pixels, size, i % size, i / size)));
        }
    }
}
