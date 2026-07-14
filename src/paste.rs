use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use color_eyre::eyre::{Result, WrapErr};

use crate::keyboard;

pub struct Paster {
    clipboard: Clipboard,
}

impl Paster {
    pub fn new() -> Result<Self> {
        Ok(Self {
            clipboard: Clipboard::new().wrap_err("could not open the clipboard")?,
        })
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        let previous = self.clipboard.get_text().ok();
        let inserted = text.to_string();
        self.clipboard
            .set_text(&inserted)
            .wrap_err("could not write the transcript to the clipboard")?;

        keyboard::post_command('v')?;

        if let Some(previous) = previous {
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(500));
                let result = Clipboard::new().and_then(|mut clipboard| {
                    if clipboard.get_text().ok().as_deref() == Some(&inserted) {
                        clipboard.set_text(previous)?;
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    tracing::warn!(%error, "could not restore clipboard after paste");
                }
            });
        }
        Ok(())
    }
}
