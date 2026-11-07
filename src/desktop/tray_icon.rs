//! The tray mark Linux and Windows share: the white Hex mark, drawn in black
//! on a light panel or taskbar, where white would vanish.

/// The mark's edge, in pixels.
pub(crate) const SIZE: u32 = 32;
/// The white Hex mark, raw 32×32 RGBA.
const RGBA: &[u8] = include_bytes!("../../resources/windows/tray-32.rgba");

/// The tray mark for a light panel or taskbar: the white mark on a dark one,
/// the same shape in black on a light one.
pub(crate) fn pixels(light: bool) -> Vec<u8> {
    let mut pixels = RGBA.to_vec();
    if light {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[..3].fill(0);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_tray_gets_a_dark_mark_with_the_same_shape() {
        let dark = pixels(false);
        let light = pixels(true);
        assert_eq!(dark, RGBA);
        assert_eq!(light.len(), (SIZE * SIZE * 4) as usize);
        for (shown, mark) in light.as_chunks::<4>().0.iter().zip(dark.as_chunks::<4>().0) {
            assert_eq!(&shown[..3], &[0, 0, 0]);
            assert_eq!(shown[3], mark[3]);
        }
    }
}
