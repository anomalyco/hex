use super::*;
use crate::paste::{ClipboardFlavor, ClipboardSnapshot};
use std::cell::{Cell, RefCell};

struct Clipboard {
    count: Cell<isize>,
    text: RefCell<String>,
}
impl Clipboard {
    fn new() -> Self {
        Self {
            count: Cell::new(10),
            text: RefCell::new("original".into()),
        }
    }
    fn copy(&self, value: &'static str) {
        self.claim();
        self.write(value).unwrap()
    }
}
impl Board for Clipboard {
    type Item = &'static str;
    fn generation(&self) -> isize {
        self.count.get()
    }
    fn save(&self) -> Result<ClipboardSnapshot> {
        Ok(ClipboardSnapshot {
            items: vec![vec![ClipboardFlavor {
                data_type: "public.utf8-plain-text".into(),
                data: self.text.borrow().as_bytes().to_vec(),
                flags: 0,
            }]],
        })
    }
    fn claim(&self) -> isize {
        self.count.set(self.count.get() + 1);
        self.count.get()
    }
    fn write(&self, value: &str) -> Result<()> {
        self.text.replace(value.into());
        Ok(())
    }
    fn restore(&self, s: &ClipboardSnapshot) -> Result<()> {
        self.claim();
        self.text
            .replace(String::from_utf8(s.items[0][0].data.clone()).unwrap());
        Ok(())
    }
}

#[test]
fn target_switch_after_first_paste_restores_the_original_without_posting_again() {
    let board = Clipboard::new();
    let focused = Cell::new(true);
    let e = transaction(
        &board,
        &mut ClipboardRestore::default(),
        Some("sheet"),
        "summary",
        || focused.get(),
        || {
            assert!(focused.replace(false));
            assert_eq!(*board.text.borrow(), "sheet");
            Ok(())
        },
        |_| {},
    )
    .unwrap_err();
    assert!(e.to_string().contains("partially pasted"));
    assert_eq!(*board.text.borrow(), "original");
}

#[test]
fn an_external_copy_after_the_sheet_stops_the_summary_and_survives_restore() {
    let board = Clipboard::new();
    let mut posts = 0;
    let e = transaction(
        &board,
        &mut ClipboardRestore::default(),
        Some("sheet"),
        "summary",
        || true,
        || {
            posts += 1;
            board.copy("external copy");
            Ok(())
        },
        |_| {},
    )
    .unwrap_err();
    assert!(e.to_string().contains("partially pasted"));
    assert_eq!(posts, 1);
    assert_eq!(*board.text.borrow(), "external copy");
}

#[test]
fn a_capture_after_dictation_restores_the_pre_dictation_clipboard() {
    let board = Clipboard::new();
    let mut pending = ClipboardRestore::default();
    let original = board.save().unwrap();
    let before = board.generation();
    board.copy("dictation");
    pending.register(Some(original), before, board.generation());
    let mut posts = vec![];
    transaction(
        &board,
        &mut pending,
        Some("sheet"),
        "summary",
        || true,
        || {
            posts.push(board.text.borrow().clone());
            Ok(())
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(posts, ["sheet", "summary"]);
    assert_eq!(*board.text.borrow(), "original");
}
