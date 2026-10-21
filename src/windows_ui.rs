//! Windows window chrome on the shared design system: Segoe Fluent Icons
//! glyphs, the custom caption bar drawn into gpui's frameless titlebar area,
//! the window and tray icons, and the HUD popup's DWM attributes.

use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, Div, FontWeight, Image, ImageFormat, Window, WindowControlArea, div, img,
    prelude::*, px, rgb,
};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWLP_HINSTANCE, GetSystemMetrics, GetWindowLongPtrW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
    LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
    WM_SETICON,
};
use winreg::RegKey;
use winreg::enums::HKEY_LOCAL_MACHINE;

use crate::design_tokens::{
    CAPTION, Typography, on_accent, surface_hover, surface_selected, text_primary, text_secondary,
};

const FONT_UI: &str = "Segoe UI Variable Text";

/// Sets the Windows UI face on the window root; every type step inherits it.
pub(crate) fn ui_font(frame: Div) -> Div {
    frame.font_family(FONT_UI)
}

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
/// The system caption buttons draw their Segoe glyphs at 10 px.
const CAPTION_GLYPH_SIZE: f32 = 10.0;

// The system caption close button's hover and pressed reds.
const CLOSE_HOVER: u32 = 0xc42b1c;
const CLOSE_PRESSED: u32 = 0xb2271a;

/// Gives the main window the executable's embedded icon (resource 1, see
/// build.rs) for the taskbar, Alt-Tab, and the caption, so they match the
/// exe in Explorer and Task Manager. GPUI registers its window class without
/// one. Windows picks the frame that fits each size and DPI.
pub(crate) fn apply_window_icon(hwnd: Option<HWND>) {
    let Some(hwnd) = hwnd else {
        return;
    };
    const APP_ICON: u16 = 1;
    unsafe {
        let module = GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) as HINSTANCE;
        for (kind, width, height) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            let icon = LoadImageW(
                module,
                APP_ICON as usize as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(width),
                GetSystemMetrics(height),
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
/// Windows 11 decorates top-level windows with rounded corners, a hairline
/// border, and a drop shadow; on the mostly transparent HUD popup they all
/// read as a ghost box floating around the capsule, so turn them off.
pub(crate) fn strip_popup_chrome(hwnd: HWND) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
    };
    // DWMWA_COLOR_NONE: no border at all (not merely transparent).
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    unsafe {
        let corner = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const corner).cast(),
            size_of_val(&corner) as u32,
        );
        let color = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            (&raw const color).cast(),
            size_of_val(&color) as u32,
        );
        // The shadow is non-client rendering; verified live that disabling
        // it removes the ghost rectangle behind the transparent HUD.
        let policy = DWMNCRP_DISABLED;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY as u32,
            (&raw const policy).cast(),
            size_of_val(&policy) as u32,
        );
    }
}

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
    include_bytes!("../resources/windows/tray-32.rgba").to_vec()
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

/// The caption row is split to match the columns below it, so the sidebar
/// runs the full window height as it does on macOS. Both halves are OS drag
/// regions (double-click maximizes), and gpui routes the window buttons
/// through WM_NCHITTEST, so hover states, Snap Layouts on the maximize
/// button, and click behavior all come from Windows itself.
///
/// This is the sidebar half: the brand, aligned with the navigation icons.
pub(crate) fn caption_brand() -> impl IntoElement {
    caption_row().child(
        div()
            .id("caption-brand")
            .window_control_area(WindowControlArea::Drag)
            .h_full()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .items_center()
            .gap_2()
            .pl_3()
            .child(brand_logo())
            .child(
                div()
                    .text_step(CAPTION)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(text_secondary()))
                    .child("Hex"),
            ),
    )
}

/// The content half of the caption row: a drag region and the window
/// buttons at the far right.
pub(crate) fn caption_controls(window: &Window) -> impl IntoElement {
    let maximize_glyph = if window.is_maximized() {
        "\u{E923}"
    } else {
        "\u{E922}"
    };
    caption_row()
        .child(
            div()
                .id("caption-drag")
                .window_control_area(WindowControlArea::Drag)
                .h_full()
                .flex_1()
                .min_w(px(0.0)),
        )
        .child(caption_button(
            "caption-minimize",
            WindowControlArea::Min,
            "\u{E921}",
            surface_hover(),
            surface_selected(),
            text_primary(),
        ))
        .child(caption_button(
            "caption-maximize",
            WindowControlArea::Max,
            maximize_glyph,
            surface_hover(),
            surface_selected(),
            text_primary(),
        ))
        .child(caption_button(
            "caption-close",
            WindowControlArea::Close,
            "\u{E8BB}",
            CLOSE_HOVER,
            CLOSE_PRESSED,
            on_accent(),
        ))
}

/// The caption row blocks the window's own mouse handling beneath it (such as
/// focus-on-click), which would otherwise claim the press before Windows can
/// start a drag, snap, or caption-button click.
fn caption_row() -> Div {
    div()
        .occlude()
        .w_full()
        .h(px(CAPTION_HEIGHT))
        // Leave a thin non-interactive strip at the very top so the OS
        // top-edge resize hit test stays reachable above the drag and
        // caption-button hitboxes.
        .pt(px(3.0))
        .flex_none()
        .flex()
        .items_center()
}

/// The branded logo at 16 points, rendered from the 64px raster for crisp
/// results on high-DPI displays.
fn brand_logo() -> AnyElement {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    let image = LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../resources/windows/icon-64.png").to_vec(),
        ))
    });
    img(image.clone()).size(px(16.0)).into_any_element()
}

fn caption_button(
    id: &'static str,
    area: WindowControlArea,
    glyph: &'static str,
    hover: u32,
    active: u32,
    hover_text: u32,
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
        .text_size(px(CAPTION_GLYPH_SIZE))
        .text_color(rgb(text_secondary()))
        .hover(move |button| button.bg(rgb(hover)).text_color(rgb(hover_text)))
        .active(move |button| button.bg(rgb(active)).text_color(rgb(hover_text)))
        .child(glyph)
}
