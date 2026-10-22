//! The desktop design tokens: the only place raw colours, type sizes, line
//! heights, radii, and control heights are written down. Panes and components
//! name these tokens; `docs/design-system.md` explains when to use each one.

use gpui::{FontWeight, Pixels, Styled, px};

// Surfaces: the OpenCode V2 dark hierarchy, darkest (recessed) to lightest.
pub(crate) const SIDEBAR: u32 = 0x111111;
pub(crate) const SIDEBAR_HOVER: u32 = 0x1b1b1b;
pub(crate) const CANVAS: u32 = 0x161616;
pub(crate) const SURFACE: u32 = 0x1d1d1d;
pub(crate) const SURFACE_HOVER: u32 = 0x252525;
pub(crate) const SURFACE_SELECTED: u32 = 0x2e2e2e;
pub(crate) const LINE: u32 = 0x2a2a2a;
pub(crate) const LINE_STRONG: u32 = 0x3d3d3d;
/// Modal backdrops.
pub(crate) const SCRIM: u32 = 0x000000cc;

// Text, strongest to weakest. Tertiary is the weakest colour that still meets
// 4.5:1 on every surface; disabled is reserved for unavailable controls.
pub(crate) const TEXT_PRIMARY: u32 = 0xf2f2f2;
pub(crate) const TEXT_SECONDARY: u32 = 0xd0d0d0;
pub(crate) const TEXT_TERTIARY: u32 = 0x969696;
pub(crate) const TEXT_DISABLED: u32 = 0x6e6e6e;

// Roles. Each has a text/fill colour, a tinted surface, and a border.
pub(crate) const ACCENT: u32 = 0x3b5cf6;
pub(crate) const ACCENT_TEXT: u32 = 0x8390ff;
pub(crate) const ACCENT_SURFACE: u32 = 0x1c2030;
pub(crate) const ACCENT_BORDER: u32 = 0x2c3560;
/// Text selection highlight: the accent at one-third opacity.
pub(crate) const SELECTION: u32 = 0x3b5cf655;
pub(crate) const SUCCESS: u32 = 0x69d89f;
pub(crate) const SUCCESS_SURFACE: u32 = 0x17231a;
pub(crate) const DANGER: u32 = 0xf17471;
pub(crate) const DANGER_SURFACE: u32 = 0x2b1718;
pub(crate) const DANGER_BORDER: u32 = 0x66292c;

/// One step of the type scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TypeStep {
    pub(crate) size: f32,
    pub(crate) line_height: f32,
    /// `None` inherits the surrounding weight.
    pub(crate) weight: Option<FontWeight>,
}

/// Metadata, timestamps, tags, and section labels.
pub(crate) const CAPTION: TypeStep = TypeStep {
    size: 11.0,
    line_height: 14.0,
    weight: None,
};
/// Descriptions, control labels, and secondary copy.
pub(crate) const FOOTNOTE: TypeStep = TypeStep {
    size: 12.0,
    line_height: 16.0,
    weight: None,
};
/// The window default: values, inputs, list titles, and reading text.
pub(crate) const BODY: TypeStep = TypeStep {
    size: 13.0,
    line_height: 18.0,
    weight: None,
};
/// Detail, card, and dialog titles.
pub(crate) const HEADLINE: TypeStep = TypeStep {
    size: 15.0,
    line_height: 20.0,
    weight: Some(FontWeight::SEMIBOLD),
};
/// Pane titles.
pub(crate) const TITLE: TypeStep = TypeStep {
    size: 20.0,
    line_height: 26.0,
    weight: Some(FontWeight::SEMIBOLD),
};

/// The only text below the scale: the L/R badge in a sided modifier keycap.
pub(crate) const KEYCAP_SIDE_BADGE: TypeStep = TypeStep {
    size: 7.0,
    line_height: 8.0,
    weight: Some(FontWeight::SEMIBOLD),
};

pub(crate) const MONOSPACE_FAMILY: &str = "SF Mono";

/// SF Pro's ascender and descender as fractions of the font size.
const FONT_ASCENT: f32 = 0.952;
const FONT_DESCENT: f32 = 0.241;

/// The top inset that puts `smaller`'s first baseline on `larger`'s when both
/// lines start at the same top, such as a timestamp beside a row title.
pub(crate) fn baseline_offset(larger: TypeStep, smaller: TypeStep) -> f32 {
    // GPUI centres the font's ascent + descent in the line box.
    let baseline =
        |step: TypeStep| step.line_height / 2.0 + step.size * (FONT_ASCENT - FONT_DESCENT) / 2.0;
    baseline(larger) - baseline(smaller)
}

/// Applies type steps to any styled element. Weight is applied only by steps
/// that own one, so call `font_weight` after the step to emphasise text.
pub(crate) trait Typography: Styled + Sized {
    fn text_step(self, step: TypeStep) -> Self {
        let styled = self
            .text_size(px(step.size))
            .line_height(px(step.line_height));
        match step.weight {
            Some(weight) => styled.font_weight(weight),
            None => styled,
        }
    }

    fn text_caption(self) -> Self {
        self.text_step(CAPTION)
    }

    fn text_footnote(self) -> Self {
        self.text_step(FOOTNOTE)
    }

    fn text_body(self) -> Self {
        self.text_step(BODY)
    }

    fn text_headline(self) -> Self {
        self.text_step(HEADLINE)
    }

    fn text_title(self) -> Self {
        self.text_step(TITLE)
    }

    fn text_monospace(self) -> Self {
        self.font_family(MONOSPACE_FAMILY)
    }
}

impl<E: Styled> Typography for E {}

// Spacing: a 4 px grid. Prefer GPUI's spacing utilities (`*_1` 4, `*_2` 8,
// `*_3` 12, `*_4` 16, `*_5` 20, `*_6` 24, `*_8` 32); `SPACE_2` is named for
// arithmetic on control heights and negative icon-button margins.
pub(crate) const SPACE_2: f32 = 8.0;

// Radii.
pub(crate) const RADIUS_SMALL: Pixels = px(4.0);
pub(crate) const RADIUS_CONTROL: Pixels = px(6.0);
pub(crate) const RADIUS_PANEL: Pixels = px(10.0);
pub(crate) const RADIUS_MODAL: Pixels = px(12.0);
pub(crate) const RADIUS_KNOB: Pixels = px(2.0);

/// The corner radius of an application icon or monogram of `size`.
pub(crate) fn icon_radius(size: f32) -> Pixels {
    px(size * 0.22)
}

// Control geometry.
pub(crate) const CONTROL_HEIGHT: f32 = 30.0;
pub(crate) const CONTROL_HEIGHT_SMALL: f32 = 24.0;
pub(crate) const KEYCAP_WIDTH: f32 = 34.0;
/// The narrowest a settings row's title and description get before its
/// control wraps below them.
pub(crate) const SETTINGS_COPY_MIN_WIDTH: f32 = 180.0;
pub(crate) const TAG_HEIGHT: f32 = 18.0;
pub(crate) const MENU_ITEM_HEIGHT: f32 = 28.0;
pub(crate) const MENU_WIDTH: f32 = 180.0;
/// The gap between a control and the menu it opens.
pub(crate) const MENU_OFFSET: f32 = 4.0;
pub(crate) const NAV_ITEM_HEIGHT: f32 = 32.0;
pub(crate) const MULTILINE_INPUT_HEIGHT: f32 = 76.0;
pub(crate) const DISCLOSURE_WIDTH: f32 = 220.0;
pub(crate) const SEARCH_FIELD_WIDTH: f32 = 220.0;
/// The widest an inline field beside a row label grows.
pub(crate) const MODE_FIELD_WIDTH: f32 = 300.0;
pub(crate) const ICON_SIZE: f32 = 14.0;
pub(crate) const ICON_SIZE_SMALL: f32 = 11.0;
pub(crate) const TOGGLE_WIDTH: f32 = 28.0;
pub(crate) const TOGGLE_HEIGHT: f32 = 16.0;
/// Opacity of a control that cannot currently be used.
pub(crate) const DISABLED_OPACITY: f32 = 0.4;

// Pane geometry.
pub(crate) const SIDEBAR_WIDTH: f32 = 220.0;
/// Space above the sidebar's first item, clear of the window controls.
pub(crate) const TITLEBAR_INSET: f32 = 52.0;
pub(crate) const PANE_HEADER_HEIGHT: f32 = 70.0;
/// Horizontal inset shared by every pane header and body, so cards align
/// with the header rule.
pub(crate) const PANE_GUTTER: f32 = 32.0;
/// The one content width every pane is bounded to.
pub(crate) const PANE_CONTENT_WIDTH: f32 = 940.0;
/// The list column of a list+detail pane.
pub(crate) const PANE_LIST_WIDTH: f32 = 320.0;
/// The narrower list column for panes whose detail is an editor (Modes).
pub(crate) const PANE_LIST_WIDTH_COMPACT: f32 = 220.0;
pub(crate) const MODAL_WIDTH: f32 = 780.0;
pub(crate) const MODAL_HEIGHT: f32 = 520.0;
/// A determinate or indeterminate progress track.
pub(crate) const PROGRESS_HEIGHT: f32 = 3.0;
pub(crate) const PANEL_HEADER_HEIGHT: f32 = 38.0;
pub(crate) const SETTINGS_ROW_MIN_HEIGHT: f32 = 56.0;
/// Fixed width of a meeting row's inline Copy / Copied button.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const MEETING_ROW_COPY_WIDTH: f32 = 58.0;

// Meeting capsule geometry.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const MEETING_PANEL_WIDTH: f32 = 420.0;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// Expanded width of the floating meeting recording capsule.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const RECORDING_PANEL_WIDTH: f32 = 548.0;
/// Compact width of the floating meeting recording capsule at rest.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const RECORDING_COMPACT_WIDTH: f32 = 244.0;
/// Height of the floating meeting recording capsule window.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const RECORDING_PANEL_HEIGHT: f32 = 48.0;
/// Fixed width of the recording capsule's Copy new / Copied button.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const RECORDING_COPY_NEW_WIDTH: f32 = 108.0;
/// Fixed width of the recording capsule's Copy all / Copied button.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const RECORDING_COPY_ALL_WIDTH: f32 = 76.0;
/// Fixed width of the ready capsule's Copy / Copied button.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) const READY_COPY_WIDTH: f32 = 68.0;

// Onboarding geometry.
/// The tour's one column: page art, copy, cards, and footer share its edges.
pub(crate) const ONBOARDING_WIDTH: f32 = 520.0;
/// The illustration band above a centred page's title.
pub(crate) const ONBOARDING_ART_HEIGHT: f32 = 148.0;
/// The shortcut drawn as a large keycap in onboarding art.
pub(crate) const HERO_KEYCAP_SIZE: f32 = 56.0;
/// An icon drawn as onboarding art, such as the finished tour's check.
pub(crate) const HERO_ICON_SIZE: f32 = 28.0;
/// How far a hero keycap stands above its base before it is pressed.
pub(crate) const HERO_KEYCAP_DEPTH: f32 = 3.0;
/// The measure of a page's centred body copy.
pub(crate) const ONBOARDING_BODY_WIDTH: f32 = 400.0;
/// A setup row's download progress, track and percentage together.
pub(crate) const ONBOARDING_PROGRESS_WIDTH: f32 = 140.0;
/// The tour's main button, centred under each page.
pub(crate) const ONBOARDING_PRIMARY_WIDTH: f32 = 180.0;
/// The round badge holding a setup row's icon.
pub(crate) const ONBOARDING_BADGE_SIZE: f32 = 28.0;
/// The tour's step dots; the current one stretches to the wide width.
pub(crate) const STEP_DOT_SIZE: f32 = 6.0;
pub(crate) const STEP_DOT_ACTIVE_WIDTH: f32 = 16.0;

// Motion presets (`duration` + `bounce`, SwiftUI mapping) and transition offsets.
pub(crate) use crate::motion::Spec as SpringSpec;
#[allow(dead_code)]
pub(crate) const SPRING_INSTANT: SpringSpec = crate::motion::presets::INSTANT;
pub(crate) const SPRING_SNAP: SpringSpec = crate::motion::presets::SNAP;
#[allow(dead_code)]
pub(crate) const SPRING_SNAP_EXIT: SpringSpec = crate::motion::presets::SNAP_EXIT;
pub(crate) const SPRING_SETTLE: SpringSpec = crate::motion::presets::SETTLE;
#[allow(dead_code)]
pub(crate) const SPRING_SETTLE_EXIT: SpringSpec = crate::motion::presets::SETTLE_EXIT;
pub(crate) const SPRING_REFLOW: SpringSpec = crate::motion::presets::REFLOW;
#[allow(dead_code)]
pub(crate) const SPRING_REFLOW_EXIT: SpringSpec = crate::motion::presets::REFLOW_EXIT;
#[allow(dead_code)]
pub(crate) const SPRING_GLIDE: SpringSpec = crate::motion::presets::GLIDE;
#[allow(dead_code)]
pub(crate) const SPRING_GLIDE_EXIT: SpringSpec = crate::motion::presets::GLIDE_EXIT;
#[allow(dead_code)]
pub(crate) const SPRING_MORPH: SpringSpec = crate::motion::presets::MORPH;
#[allow(dead_code)]
pub(crate) const SPRING_MORPH_EXIT: SpringSpec = crate::motion::presets::MORPH_EXIT;
#[allow(dead_code)]
pub(crate) const SPRING_POP: SpringSpec = crate::motion::presets::POP;
#[allow(dead_code)]
pub(crate) const SPRING_POP_EXIT: SpringSpec = crate::motion::presets::POP_EXIT;
#[allow(dead_code)]
pub(crate) const SPRING_REDUCED_FADE: SpringSpec = crate::motion::presets::REDUCED_FADE;
/// Vertical travel (in px) of a pane's entrance transition when Reduce Motion is off.
pub(crate) const PANE_ENTER_OFFSET: f32 = 4.0;
/// Vertical travel (in px) of a dropdown menu opening from its trigger.
pub(crate) const MENU_ENTER_OFFSET: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(hex: u32) -> f64 {
        let channel = |shift: u32| {
            let value = f64::from((hex >> shift) & 0xff) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(foreground: u32, background: u32) -> f64 {
        let (light, dark) = {
            let (a, b) = (luminance(foreground), luminance(background));
            if a > b { (a, b) } else { (b, a) }
        };
        (light + 0.05) / (dark + 0.05)
    }

    #[test]
    fn readable_text_meets_aa_contrast_on_every_surface() {
        for background in [SIDEBAR, CANVAS, SURFACE, SURFACE_HOVER, SURFACE_SELECTED] {
            for text in [TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY] {
                let ratio = contrast(text, background);
                assert!(
                    ratio >= 4.5,
                    "{text:06x} on {background:06x} is {ratio:.2}:1"
                );
            }
        }
        for (text, surface) in [
            (ACCENT_TEXT, ACCENT_SURFACE),
            (SUCCESS, SUCCESS_SURFACE),
            (DANGER, DANGER_SURFACE),
        ] {
            let ratio = contrast(text, surface);
            assert!(ratio >= 4.5, "{text:06x} on {surface:06x} is {ratio:.2}:1");
        }
    }

    #[test]
    fn type_scale_ascends() {
        let steps = [CAPTION, FOOTNOTE, BODY, HEADLINE, TITLE];
        for pair in steps.windows(2) {
            assert!(pair[0].size < pair[1].size);
            assert!(pair[0].line_height < pair[1].line_height);
        }
        for step in steps {
            assert!(step.line_height >= step.size * 1.25);
        }
    }

    #[test]
    fn baseline_offset_lowers_smaller_text_onto_the_larger_baseline() {
        assert_eq!(baseline_offset(BODY, BODY), 0.0);
        let caption = baseline_offset(BODY, CAPTION);
        assert!((2.5..3.0).contains(&caption), "{caption}");
        assert!(baseline_offset(HEADLINE, CAPTION) > caption);
    }

    /// Raw styling values belong here. A pane that writes a literal colour,
    /// font size, line height, or radius, or truncates text by hand, bypasses
    /// the design system.
    #[test]
    fn desktop_sources_use_tokens_instead_of_raw_styles() {
        const SOURCES: &[(&str, &str)] = &[
            ("app_window.rs", include_str!("app_window.rs")),
            ("app_window/macos.rs", include_str!("app_window/macos.rs")),
            ("app_window/linux.rs", include_str!("app_window/linux.rs")),
            ("app_window/tour.rs", include_str!("app_window/tour.rs")),
            (
                "app_window/windows.rs",
                include_str!("app_window/windows.rs"),
            ),
            ("desktop_ui.rs", include_str!("desktop_ui.rs")),
            (
                "desktop_transcription_picker.rs",
                include_str!("desktop_transcription_picker.rs"),
            ),
            ("linux_app.rs", include_str!("linux_app.rs")),
            ("meeting_watcher.rs", include_str!("meeting_watcher.rs")),
            ("text_input.rs", include_str!("text_input.rs")),
        ];
        const FORBIDDEN: &[&str] = &[
            "rgb(0x",
            "rgba(0x",
            ".text_size(",
            ".line_height(px(",
            ".line_height(relative(",
            ".line_height(rems(",
            ".font_family(",
            ".rounded(px(",
            ".rounded_sm()",
            ".rounded_md()",
            ".rounded_lg()",
            ".text_xs()",
            ".text_sm()",
            ".text_base()",
            ".text_lg()",
            ".text_xl()",
            // One-line text goes through `desktop_ui::truncated_line`.
            ".truncate()",
        ];
        let mut violations = Vec::new();
        for (file, source) in SOURCES {
            for (index, line) in source.lines().enumerate() {
                for pattern in FORBIDDEN {
                    if line.contains(pattern) {
                        violations.push(format!("{file}:{}: {}", index + 1, line.trim()));
                    }
                }
            }
        }
        assert!(
            violations.is_empty(),
            "use design_tokens instead of raw styles:\n{}",
            violations.join("\n")
        );
    }
}
