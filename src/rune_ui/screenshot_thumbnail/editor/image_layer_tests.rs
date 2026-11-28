//! Tests for stacked image layers (paste, drag-and-drop, move, aspect-locked
//! resize, z-order, delete, Undo/Redo, full-res compositing) and Open in Preview.

use std::path::PathBuf;

use rune::{ClipboardContent, ClipboardImage, Key, KeyEvent, Mods, Phase, Tx};

use super::export::Export;
use super::image_layer::{
    Corner, DecodedImage, composite_all_images, decode_png_bytes, hit_corner, initial_rect,
    is_image_path, move_rect, resize_rect,
};
use super::marks::{self, Mark, Tool};
use super::tests::shot;
use super::{Editor, on_key};

fn solid_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let pixels = rgba.repeat((width * height) as usize);
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    bytes
}

fn decoded_solid(width: u32, height: u32, rgba: [u8; 4]) -> DecodedImage {
    DecodedImage {
        width,
        height,
        rgba: rgba.repeat((width * height) as usize),
    }
}

fn cmd(c: char, shift: bool) -> KeyEvent {
    KeyEvent {
        key: Key::Command(c),
        mods: Mods {
            cmd: true,
            shift,
            ..Default::default()
        },
    }
}

fn plain(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        mods: Mods::default(),
    }
}

#[test]
fn paste_clipboard_image_inserts_centered_aspect_clamped_layer() {
    let mut editor = Editor::still(shot([1000, 600], 2.0), [1440.0, 900.0]);
    let mut tx = Tx::default();
    let png = solid_png(2000, 1000, [20, 180, 90, 255]);
    let content =
        ClipboardContent::default().with_image(ClipboardImage::from_png(png.as_slice()).unwrap());
    editor.handle_clipboard(&content, &mut tx);

    assert_eq!(editor.marks.len(), 1);
    let Mark::Image(layer) = &editor.marks[0] else {
        panic!("expected pasted image layer");
    };
    assert_eq!(editor.selected_image, Some(layer.id));
    // 2000×1000 clamped to 70% of 1000×600 => 700×350, centered at (150, 125).
    let first_rect = layer.rect;
    assert_eq!(first_rect, [150.0, 125.0, 700.0, 350.0]);
    assert_eq!(layer.source_size, [2000, 1000]);

    // Pasting a second identical image cascades so both remain visible.
    editor.handle_clipboard(&content, &mut tx);
    assert_eq!(editor.marks.len(), 2);
    let Mark::Image(second) = &editor.marks[1] else {
        panic!("expected second pasted image layer");
    };
    assert_eq!(editor.selected_image, Some(second.id));
    assert!(second.rect[0] > first_rect[0] && second.rect[1] > first_rect[1]);
}

#[test]
fn drop_image_files_places_layer_at_drop_point_and_ignores_non_images() {
    let dir = std::env::temp_dir().join(format!("hex-drop-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img_path = dir.join("sticker.png");
    let txt_path = dir.join("notes.txt");
    std::fs::write(&img_path, solid_png(120, 80, [255, 80, 40, 255])).unwrap();
    std::fs::write(&txt_path, b"not an image").unwrap();

    assert!(is_image_path(&img_path));
    assert!(is_image_path(&PathBuf::from("photo.WEBP")));
    assert!(!is_image_path(&txt_path));

    let mut editor = Editor::still(shot([800, 600], 1.0), [1200.0, 800.0]);
    let mut tx = Tx::default();
    editor.drop_image_files(
        &[txt_path.clone(), img_path.clone()],
        Some([300.0, 200.0]),
        &mut tx,
    );

    assert_eq!(editor.marks.len(), 1);
    let Mark::Image(layer) = &editor.marks[0] else {
        panic!("expected dropped image layer");
    };
    // 120×80 centered at (300, 200) => top-left (240, 160).
    assert_eq!(layer.rect, [240.0, 160.0, 120.0, 80.0]);

    // Also verify Rune's headless on_drop_files handler wires through view::view.
    let mut rt = crate::rune_ui::offscreen::runtime(
        Editor::still(shot([800, 600], 1.0), [1200.0, 800.0]),
        super::view::view,
        (1200.0, 800.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    rt.frame(0.0);
    let layout = rt.state.layout();
    let drop_pt = (
        layout.panel[0] + layout.image[0] + layout.image[2] * 0.5,
        layout.panel[1] + layout.image[1] + layout.image[3] * 0.5,
    );
    rt.drag_files(vec![img_path.clone()], Some(drop_pt));
    rt.drop_files(vec![img_path], Some(drop_pt));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.marks.len(), 1);
    assert!(matches!(rt.state.marks[0], Mark::Image(_)));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn move_and_aspect_locked_corner_resize_preserve_anchor_and_ratio() {
    let start = [100.0, 100.0, 200.0, 100.0]; // 2:1 aspect ratio
    let canvas = [800, 600];

    // Move freely, clamped so it never vanishes off-canvas.
    assert_eq!(
        move_rect(start, [45.0, -30.0], canvas, 1.0),
        [145.0, 70.0, 200.0, 100.0]
    );
    let off = move_rect(start, [-500.0, 900.0], canvas, 1.0);
    assert!(off[0] + off[2] >= 16.0 && off[1] <= 600.0 - 16.0);

    // Every corner preserves 2:1 ratio and keeps its opposite corner fixed.
    let br = resize_rect(
        start,
        Corner::BottomRight,
        [400.0, 230.0],
        canvas,
        1.0,
        false,
    );
    assert_eq!([br[0], br[1]], [100.0, 100.0], "top-left stays anchored");
    assert_eq!([br[2], br[3]], [300.0, 150.0], "2:1 ratio preserved");

    let tl = resize_rect(start, Corner::TopLeft, [0.0, 50.0], canvas, 1.0, false);
    assert_eq!(
        [tl[0] + tl[2], tl[1] + tl[3]],
        [300.0, 200.0],
        "bottom-right stays anchored"
    );
    assert!(
        (tl[2] / tl[3] - 2.0).abs() < 0.02,
        "2:1 ratio preserved: {tl:?}"
    );

    let tr = resize_rect(start, Corner::TopRight, [400.0, 80.0], canvas, 1.0, false);
    assert_eq!(
        [tr[0], tr[1] + tr[3]],
        [100.0, 200.0],
        "bottom-left stays anchored"
    );
    assert!(
        (tr[2] / tr[3] - 2.0).abs() < 0.02,
        "2:1 ratio preserved: {tr:?}"
    );

    let bl = resize_rect(start, Corner::BottomLeft, [50.0, 250.0], canvas, 1.0, false);
    assert_eq!(
        [bl[0] + bl[2], bl[1]],
        [300.0, 100.0],
        "top-right stays anchored"
    );
    assert!(
        (bl[2] / bl[3] - 2.0).abs() < 0.02,
        "2:1 ratio preserved: {bl:?}"
    );

    // Free resize unlocks aspect ratio when requested.
    let free = resize_rect(
        start,
        Corner::BottomRight,
        [400.0, 350.0],
        canvas,
        1.0,
        true,
    );
    assert_eq!(free, [100.0, 100.0, 300.0, 250.0]);

    // Corner hit testing picks the exact corner within reach.
    assert_eq!(
        hit_corner(start, [102.0, 101.0], 10.0),
        Some(Corner::TopLeft)
    );
    assert_eq!(
        hit_corner(start, [298.0, 199.0], 10.0),
        Some(Corner::BottomRight)
    );
    assert_eq!(hit_corner(start, [200.0, 150.0], 10.0), None);
}

#[test]
fn z_order_selection_move_resize_delete_and_undo_redo() {
    let mut editor = Editor::still(shot([800, 600], 1.0), [1200.0, 800.0]);
    let mut tx = Tx::default();

    // Insert layer A at (100, 100) size 120×80, and layer B overlapping at (160, 120) size 120×80.
    let id_a = editor
        .insert_decoded_image(
            decoded_solid(120, 80, [255, 0, 0, 255]),
            Some([160.0, 140.0]),
        )
        .unwrap();
    let id_b = editor
        .insert_decoded_image(
            decoded_solid(120, 80, [0, 255, 0, 255]),
            Some([220.0, 160.0]),
        )
        .unwrap();
    assert_eq!(editor.selected_image, Some(id_b));
    let order = |e: &Editor| -> Vec<u64> {
        e.marks
            .iter()
            .filter_map(|m| match m {
                Mark::Image(i) => Some(i.id),
                _ => None,
            })
            .collect()
    };
    assert_eq!(order(&editor), vec![id_a, id_b]);

    // Click & drag layer A's non-overlapping region: brings A on top and moves it by (+30, +20).
    editor.handle_canvas_drag(Phase::Down, [115.0, 110.0], 1.0);
    editor.handle_canvas_drag(Phase::Move, [145.0, 130.0], 1.0);
    editor.handle_canvas_drag(Phase::Up, [145.0, 130.0], 1.0);
    assert_eq!(editor.selected_image, Some(id_a));
    assert_eq!(order(&editor), vec![id_b, id_a]);
    let moved_rect = editor.image_mark(id_a).unwrap().rect;
    assert_eq!(moved_rect, [130.0, 120.0, 120.0, 80.0]);

    // Drag bottom-right corner of A from (250, 200) to (310, 240) => resizes to 180×120.
    editor.handle_canvas_drag(Phase::Down, [250.0, 200.0], 1.0);
    editor.handle_canvas_drag(Phase::Move, [310.0, 240.0], 1.0);
    editor.handle_canvas_drag(Phase::Up, [310.0, 240.0], 1.0);
    assert_eq!(
        editor.image_mark(id_a).unwrap().rect,
        [130.0, 120.0, 180.0, 120.0]
    );

    // Delete key removes selected layer A.
    assert!(on_key(&mut editor, &plain(Key::Delete), &mut tx));
    assert_eq!(order(&editor), vec![id_b]);
    assert_eq!(editor.selected_image, None);

    // ⌘Z undoes Delete -> A is back at resized rect.
    assert!(on_key(&mut editor, &cmd('z', false), &mut tx));
    assert_eq!(order(&editor), vec![id_b, id_a]);
    assert_eq!(editor.selected_image, Some(id_a));
    assert_eq!(
        editor.image_mark(id_a).unwrap().rect,
        [130.0, 120.0, 180.0, 120.0]
    );

    // ⌘Z undoes Resize -> A is back at moved rect.
    assert!(on_key(&mut editor, &cmd('z', false), &mut tx));
    assert_eq!(
        editor.image_mark(id_a).unwrap().rect,
        [130.0, 120.0, 120.0, 80.0]
    );

    // ⇧⌘Z redoes Resize -> A is 180×120 again.
    assert!(on_key(&mut editor, &cmd('z', true), &mut tx));
    assert_eq!(
        editor.image_mark(id_a).unwrap().rect,
        [130.0, 120.0, 180.0, 120.0]
    );

    // Escape deselects the image without closing the editor; second Escape closes it.
    assert!(on_key(&mut editor, &plain(Key::Escape), &mut tx));
    assert!(editor.on, "first Escape only deselects the image");
    assert_eq!(editor.selected_image, None);

    // Clicking an unselected image selects it without leaving a stray Pen mark.
    editor.tool = Tool::Pen;
    editor.handle_canvas_drag(Phase::Down, [150.0, 150.0], 1.0);
    editor.handle_canvas_drag(Phase::Up, [150.0, 150.0], 1.0);
    assert_eq!(editor.selected_image, Some(id_a));
    assert_eq!(
        editor.marks.len(),
        2,
        "no stray pen dot added on image click"
    );

    // Backspace also removes the selected image.
    assert!(on_key(&mut editor, &plain(Key::Backspace), &mut tx));
    assert_eq!(order(&editor), vec![id_b]);
}

#[test]
fn full_resolution_export_composites_stacked_and_resized_images_at_exact_coordinates() {
    // Base shot: solid dark blue 200×120.
    let base_rgba = [10u8, 20, 40, 255].repeat(200 * 120);
    let base_arc = std::sync::Arc::new(base_rgba);
    let base_image =
        rune::ImageCache::new(std::sync::Arc::new(|| {})).rgba(200, 120, (*base_arc).clone());
    base_image.wait().unwrap();

    // Layer 1: solid opaque red 20×10, resized 2× to 40×20 at (30, 20).
    let mut editor = Editor::still(
        super::Shot {
            image: base_image.clone(),
            rgba: base_arc.clone(),
            size: [200, 120],
            scale: 1.0,
        },
        [800.0, 600.0],
    );
    let id1 = editor
        .insert_decoded_image(
            decoded_solid(20, 10, [240, 20, 20, 255]),
            Some([50.0, 30.0]),
        )
        .unwrap();
    editor.image_mark_mut(id1).unwrap().rect = [30.0, 20.0, 40.0, 20.0];

    // Layer 2: 50%-alpha green 20×20 placed at (50, 25), overlapping Layer 1 on x=[50..70), y=[25..40).
    let id2 = editor
        .insert_decoded_image(
            decoded_solid(20, 20, [20, 220, 20, 128]),
            Some([60.0, 35.0]),
        )
        .unwrap();
    editor.image_mark_mut(id2).unwrap().rect = [50.0, 25.0, 20.0, 20.0];

    // Export with a crop [10, 10, 100, 60] => Layer 1 sits at crop (20..60, 10..30), Layer 2 at (40..60, 15..35).
    let export = Export {
        image: base_image,
        rgba: base_arc,
        size: [200, 120],
        scale: 1.0,
        marks: editor.marks.clone(),
        crop: Some([10.0, 10.0, 100.0, 60.0]),
    };
    let png = export.png().unwrap();
    let decoded = decode_png_bytes(&png).unwrap();
    assert_eq!((decoded.width, decoded.height), (100, 60));

    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * decoded.width + x) * 4) as usize;
        [
            decoded.rgba[i],
            decoded.rgba[i + 1],
            decoded.rgba[i + 2],
            decoded.rgba[i + 3],
        ]
    };

    // Outside both layers: untouched base dark blue.
    assert_eq!(pixel(5, 5), [10, 20, 40, 255]);
    // Inside Layer 1 only: crop (25, 15) = base (35, 25) -> opaque red.
    assert_eq!(pixel(25, 15), [240, 20, 20, 255]);
    // Inside Layer 2 over Layer 1: crop (50, 20) = base (60, 30) -> ~50% green over red.
    let overlap = pixel(50, 20);
    assert!(
        overlap[0].abs_diff(130) <= 3 && overlap[1].abs_diff(120) <= 3 && overlap[3] == 255,
        "expected ~50% green blended over red, got {overlap:?}"
    );
    // Inside Layer 2 over base only: crop (50, 32) = base (60, 42) -> ~50% green over dark blue.
    let green_over_base = pixel(50, 32);
    assert!(
        green_over_base[0].abs_diff(15) <= 3
            && green_over_base[1].abs_diff(120) <= 3
            && green_over_base[2].abs_diff(30) <= 3,
        "expected ~50% green over dark blue, got {green_over_base:?}"
    );

    // Direct CPU helper matches Export::png() output byte-for-byte.
    let direct = composite_all_images(
        &export.rgba,
        export.size,
        marks::pixel_rect(export.crop, export.size),
        &export.marks,
    );
    assert_eq!(direct, decoded.rgba);
}

#[test]
fn initial_rect_clamps_huge_images_and_grows_tiny_ones_while_keeping_aspect() {
    let huge = initial_rect([1000, 800], [4000, 2000], None, &[], 1.0);
    assert!(huge[2] <= 700.0 && huge[3] <= 560.0);
    assert!((huge[2] / huge[3] - 2.0).abs() < 0.02);

    let tiny = initial_rect([1000, 800], [4, 2], None, &[], 1.0);
    assert!(tiny[2] >= 24.0);
    assert!((tiny[2] / tiny[3] - 2.0).abs() < 0.05);
}

#[test]
fn open_in_preview_command_targets_preview_on_macos_and_viewer_elsewhere() {
    let path = PathBuf::from("/tmp/Hex Screenshot 2026-11-27 at 10.00.00.png");
    let cmd = super::super::files::open_in_preview_command(&path);
    let args: Vec<_> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    #[cfg(target_os = "macos")]
    {
        assert_eq!(cmd.get_program(), "/usr/bin/open");
        assert_eq!(args, vec!["-a", "Preview", path.to_string_lossy().as_ref()]);
        assert_eq!(super::super::files::preview_label(), "Open in Preview");
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        assert_eq!(cmd.get_program(), "xdg-open");
        assert_eq!(args, vec![path.to_string_lossy().as_ref()]);
        assert_eq!(
            super::super::files::preview_label(),
            "Open in default viewer"
        );
    }
}

#[test]
fn cmd_v_requests_clipboard_read_only_when_not_typing_or_sending() {
    let mut editor = Editor::still(shot([800, 600], 2.0), [1200.0, 800.0]);
    let mut tx = Tx::default();
    assert!(on_key(&mut editor, &cmd('v', false), &mut tx));

    // While typing text, ⌘V does not hijack the label to paste an image.
    editor.typing = Some(([20.0, 20.0], "hello".into()));
    let mut tx2 = Tx::default();
    assert!(!on_key(&mut editor, &cmd('v', false), &mut tx2));
}

#[test]
fn mixed_image_and_vector_export_preserves_z_order_on_gpu() {
    if rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).is_err() {
        eprintln!("skipping mixed GPU export test: no GPU adapter");
        return;
    }
    let base = shot([300, 200], 1.0);
    let mut editor = Editor::still(
        super::Shot {
            image: base.image.clone(),
            rgba: base.rgba.clone(),
            size: base.size,
            scale: 1.0,
        },
        [800.0, 600.0],
    );
    let id = editor
        .insert_decoded_image(
            decoded_solid(100, 100, [20, 200, 40, 255]),
            Some([150.0, 100.0]),
        )
        .unwrap();
    editor.image_mark_mut(id).unwrap().rect = [100.0, 50.0, 100.0, 100.0];
    // Draw a red box right across the pasted green image so z-order puts red over green.
    editor.marks.push(Mark::Box {
        from: [120.0, 70.0],
        to: [180.0, 130.0],
    });
    let export = Export {
        image: base.image,
        rgba: base.rgba,
        size: base.size,
        scale: 1.0,
        marks: editor.marks,
        crop: None,
    };
    let rgba = export.render([0, 0, 300, 200]).unwrap();
    let at = |x: usize, y: usize| &rgba[(y * 300 + x) * 4..(y * 300 + x) * 4 + 4];
    // Inside green layer, away from red box edge: green!
    let green = at(110, 60);
    assert!(
        green[1] > 180 && green[0] < 60,
        "expected green layer pixel, got {green:?}"
    );
    // On red box top edge over the green layer: red on top of green!
    let red = at(150, 70);
    assert!(
        red[0] > 180 && red[1] < 90,
        "expected red box above image layer, got {red:?}"
    );
}
