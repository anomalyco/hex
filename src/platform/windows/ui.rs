//! Windows 11 Fluent visual layer: the user's system accent color, Segoe
//! Fluent Icons glyphs, and the custom caption bar drawn into gpui's
//! frameless titlebar area.

use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, Div, FontWeight, Image, ImageFormat, Window, WindowControlArea, div, img,
    prelude::*, px, rgb,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SendMessageW, WM_SETICON,
};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

pub(crate) const FONT_UI: &str = "Segoe UI Variable Text";

/// Segoe Fluent Icons ships with Windows 11; Windows 10 carries the same
/// glyphs we use in Segoe MDL2 Assets.
pub(crate) fn icon_font() -> &'static str {
    static FONT: OnceLock<&'static str> = OnceLock::new();
    FONT.get_or_init(|| {
        let build: u32 = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|key| key.get_value::<String, _>("CurrentBuild"))
            .ok()
            .and_then(|build| build.parse().ok())
            .unwrap_or(22000);
        if build >= 22000 {
            "Segoe Fluent Icons"
        } else {
            "Segoe MDL2 Assets"
        }
    })
}

pub(crate) const CAPTION_HEIGHT: f32 = 40.0;
const CAPTION_BUTTON_WIDTH: f32 = 46.0;

// Fluent dark control fills live in the shared token set; re-exported so
// Windows-only code keeps its historical paths.
pub(crate) use crate::desktop_ui::{
    CONTROL_FILL, CONTROL_FILL_PRESSED, CONTROL_STROKE, CRITICAL, DIALOG_STROKE, SUCCESS,
};
const CLOSE_HOVER: u32 = 0xc42b1c;
const CLOSE_PRESSED: u32 = 0xb2271a;

/// The user's dark-theme accent (`light2` palette slot), read once from the
/// registry. Windows precomputes the lightened dark-theme variant, so no color
/// math is required. Falls back to the Windows 11 default accent.
pub(crate) fn accent() -> u32 {
    static ACCENT: OnceLock<u32> = OnceLock::new();
    *ACCENT.get_or_init(|| read_accent_light2().unwrap_or(0x4cc2ff))
}

fn read_accent_light2() -> Option<u32> {
    let palette = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent")
        .ok()?
        .get_raw_value("AccentPalette")
        .ok()?;
    let light2 = palette.bytes.get(4..7)?;
    Some(u32::from(light2[0]) << 16 | u32::from(light2[1]) << 8 | u32::from(light2[2]))
}

/// Give the main window the branded app icon for the taskbar, Alt-Tab, and
/// the caption. The exe embeds no icon resource, so without this Windows
/// shows the generic window glyph. PNG bytes are valid modern icon resources.
pub(crate) fn apply_window_icon(hwnd: Option<HWND>) {
    let Some(hwnd) = hwnd else {
        return;
    };
    const SMALL: &[u8] = include_bytes!("../../../resources/windows/icon-32.png");
    const BIG: &[u8] = include_bytes!("../../../resources/windows/icon-64.png");
    for (bytes, size, kind) in [(SMALL, 16, ICON_SMALL), (BIG, 32, ICON_BIG)] {
        unsafe {
            let icon = CreateIconFromResourceEx(
                bytes.as_ptr(),
                bytes.len() as u32,
                1,
                0x0003_0000,
                size,
                size,
                LR_DEFAULTCOLOR,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}

/// GPUI's Transparent window background enables the legacy
/// TRANSPARENTGRADIENT accent policy, which paints a faint veil over the
/// whole window rectangle. DirectComposition already provides per-pixel
/// alpha, so switch the policy back to disabled for the HUD.
pub(crate) fn disable_window_tint(hwnd: HWND) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation_id: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut core::ffi::c_void,
        size: usize,
    }
    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut CompositionData) -> i32;
    unsafe {
        let Ok(user32) = libloading::Library::new("user32.dll") else {
            return;
        };
        let Ok(set_attribute) =
            user32.get::<SetWindowCompositionAttribute>(b"SetWindowCompositionAttribute\0")
        else {
            return;
        };
        let mut policy = AccentPolicy {
            state: 0,
            flags: 0,
            gradient: 0,
            animation_id: 0,
        };
        let mut data = CompositionData {
            attribute: 19,
            data: (&raw mut policy).cast(),
            size: size_of::<AccentPolicy>(),
        };
        set_attribute(hwnd, &mut data);
    }
}

/// The 32×32 system-tray icon: the app icon pre-rendered to raw RGBA at
/// build time from app/AppIcon.icon/Assets/Image.png (full color, so both
/// taskbar themes use the same pixels).
pub(crate) const TRAY_ICON_SIZE: u32 = 32;

pub(crate) fn tray_icon_rgba() -> Vec<u8> {
    include_bytes!("../../../resources/windows/tray-32.rgba").to_vec()
}

/// One Segoe Fluent Icons glyph, tinted and sized like text.
pub(crate) fn fluent_icon(glyph: &'static str, size: f32, color: u32) -> AnyElement {
    div()
        .flex_none()
        .font_family(icon_font())
        .text_size(px(size))
        .text_color(rgb(color))
        .child(glyph)
        .into_any_element()
}

pub(crate) use crate::desktop_ui::selection_pill;

/// The custom caption bar: brand at the left, one OS drag region across, and
/// native-behaving minimize/maximize/close buttons. gpui routes the buttons
/// through WM_NCHITTEST, so hover states, Snap Layouts on the maximize
/// button, and click behavior all come from Windows itself.
pub(crate) fn caption_bar(window: &Window) -> Div {
    let maximize_glyph = if window.is_maximized() {
        "\u{E923}"
    } else {
        "\u{E922}"
    };
    div()
        .w_full()
        .h(px(CAPTION_HEIGHT))
        // Leave a thin non-interactive strip at the very top so the OS
        // top-edge resize hit test stays reachable above the drag and
        // caption-button hitboxes.
        .pt(px(3.0))
        .flex_none()
        .flex()
        .items_center()
        .child(
            div()
                .id("caption-drag")
                .window_control_area(WindowControlArea::Drag)
                .h_full()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .gap_2()
                .pl(px(18.0))
                .child(brand_logo())
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xcfcfcf))
                        .child("HEX"),
                ),
        )
        .child(caption_button(
            "caption-minimize",
            WindowControlArea::Min,
            "\u{E921}",
        ))
        .child(caption_button(
            "caption-maximize",
            WindowControlArea::Max,
            maximize_glyph,
        ))
        .child(
            div()
                .id("caption-close")
                .window_control_area(WindowControlArea::Close)
                .w(px(CAPTION_BUTTON_WIDTH))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .font_family(icon_font())
                .text_size(px(10.0))
                .text_color(rgb(0xcfcfcf))
                .hover(|button| button.bg(rgb(CLOSE_HOVER)).text_color(rgb(0xffffff)))
                .active(|button| button.bg(rgb(CLOSE_PRESSED)).text_color(rgb(0xffffff)))
                .child("\u{E8BB}"),
        )
}

/// The branded logo at 16 points, rendered from the 64px raster for crisp
/// results on high-DPI displays.
fn brand_logo() -> AnyElement {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../resources/windows/icon-64.png").to_vec(),
        ))
    });
    img(image.clone()).size(px(16.0)).into_any_element()
}

fn caption_button(
    id: &'static str,
    area: WindowControlArea,
    glyph: &'static str,
) -> impl IntoElement {
    div()
        .id(id)
        .window_control_area(area)
        .w(px(CAPTION_BUTTON_WIDTH))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(icon_font())
        .text_size(px(10.0))
        .text_color(rgb(0xcfcfcf))
        .hover(|button| button.bg(rgb(CONTROL_FILL)).text_color(rgb(0xffffff)))
        .active(|button| button.bg(rgb(CONTROL_FILL_PRESSED)))
        .child(glyph)
}
