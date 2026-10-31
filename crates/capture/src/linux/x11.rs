//! Native X11 screen capture: the root window's pixels through MIT-SHM (`XShmGetImage`), or
//! `GetImage` where shared memory isn't offered, with the pointer drawn in from XFixes. It
//! needs no portal and no PipeWire, so it works on any X11 desktop (i3 and friends ship no
//! ScreenCast backend). Frames are 32-bit BGRX rows, the server's own layout on every
//! little-endian X server Hex runs on.

use anyhow::{Context, Result, bail, ensure};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};
use x11rb::rust_connection::RustConnection;

/// Whether this is an X11 session: `WAYLAND_DISPLAY` empty and `DISPLAY` set. A Wayland
/// session with XWayland is never X11 here, as Hex's own session detection has it.
pub fn session() -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    !set("WAYLAND_DISPLAY") && set("DISPLAY")
}

/// A rectangle on the root window, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    /// `rect` clipped to `bounds`, with even sides (x264's 4:2:0 wants them); `None` when
    /// nothing of it is left.
    fn clipped(self, bounds: Rect) -> Option<Rect> {
        let x0 = i32::from(self.x).max(i32::from(bounds.x));
        let y0 = i32::from(self.y).max(i32::from(bounds.y));
        let x1 = (i32::from(self.x) + i32::from(self.width))
            .min(i32::from(bounds.x) + i32::from(bounds.width));
        let y1 = (i32::from(self.y) + i32::from(self.height))
            .min(i32::from(bounds.y) + i32::from(bounds.height));
        let (width, height) = ((x1 - x0) & !1, (y1 - y0) & !1);
        (width >= 2 && height >= 2).then(|| Rect {
            x: x0 as i16,
            y: y0 as i16,
            width: width as u16,
            height: height as u16,
        })
    }
}

/// A shared-memory segment the server writes each frame into.
struct Shm {
    seg: u32,
    id: i32,
    addr: *mut u8,
    len: usize,
}

impl Drop for Shm {
    fn drop(&mut self) {
        unsafe {
            libc::shmdt(self.addr.cast());
        }
    }
}

// The segment is only touched by the thread that owns the grabber.
unsafe impl Send for Shm {}

/// Grabs one rectangle of the screen, frame after frame.
pub struct Grabber {
    conn: RustConnection,
    root: u32,
    pub rect: Rect,
    shm: Option<Shm>,
    cursor: bool,
}

impl Grabber {
    /// The monitor under the pointer (`area`: `None`), or `area` on the root window, clipped to
    /// the screen.
    pub fn open(area: Option<Rect>) -> Result<Self> {
        let (conn, screen) = x11rb::connect(None).context("could not open the X11 display")?;
        let root_screen = &conn.setup().roots[screen];
        let root = root_screen.root;
        ensure!(
            root_screen.root_depth == 24 || root_screen.root_depth == 32,
            "the X11 screen's depth is {}, not 24 or 32 bits",
            root_screen.root_depth
        );
        ensure!(
            conn.setup()
                .pixmap_formats
                .iter()
                .any(|f| f.depth == root_screen.root_depth && f.bits_per_pixel == 32),
            "the X11 screen doesn't store 32-bit pixels"
        );
        let screen_rect = Rect {
            x: 0,
            y: 0,
            width: root_screen.width_in_pixels,
            height: root_screen.height_in_pixels,
        };
        let rect = match area {
            Some(area) => area
                .clipped(screen_rect)
                .context("the area is off the X11 screen")?,
            None => monitor_under_pointer(&conn, root)
                .unwrap_or(screen_rect)
                .clipped(screen_rect)
                .context("the X11 screen is empty")?,
        };
        let cursor = conn
            .xfixes_query_version(5, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some();
        let mut grabber = Self {
            conn,
            root,
            rect,
            shm: None,
            cursor,
        };
        grabber.shm = grabber.attach_shm().ok();
        Ok(grabber)
    }

    fn attach_shm(&self) -> Result<Shm> {
        self.conn.shm_query_version()?.reply()?;
        let len = usize::from(self.rect.width) * usize::from(self.rect.height) * 4;
        let id = unsafe { libc::shmget(libc::IPC_PRIVATE, len, libc::IPC_CREAT | 0o600) };
        ensure!(id >= 0, "shmget failed");
        let addr = unsafe { libc::shmat(id, std::ptr::null(), 0) };
        if addr as isize == -1 {
            unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
            bail!("shmat failed");
        }
        let seg = self.conn.generate_id()?;
        let attached = self
            .conn
            .shm_attach(seg, id as u32, false)
            .map_err(anyhow::Error::from)
            .and_then(|cookie| Ok(cookie.check()?));
        // Removed now, it goes once both sides detach, even if Hex dies.
        unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
        let shm = Shm {
            seg,
            id,
            addr: addr.cast(),
            len,
        };
        attached.context("the X server refused the shared-memory segment")?;
        let _ = shm.id;
        Ok(shm)
    }

    /// The next frame into `pixels` (BGRX rows, `rect.width * 4` bytes each), the pointer
    /// drawn in.
    pub fn grab(&mut self, pixels: &mut Vec<u8>) -> Result<()> {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.rect;
        let len = usize::from(width) * usize::from(height) * 4;
        pixels.resize(len, 0);
        let shm_grabbed = match &self.shm {
            Some(shm) => self
                .conn
                .shm_get_image(
                    self.root,
                    x,
                    y,
                    width,
                    height,
                    !0,
                    ImageFormat::Z_PIXMAP.into(),
                    shm.seg,
                    0,
                )
                .map_err(anyhow::Error::from)
                .and_then(|cookie| Ok(cookie.reply()?))
                .map(|_| {
                    let src = unsafe { std::slice::from_raw_parts(shm.addr, shm.len.min(len)) };
                    pixels[..src.len()].copy_from_slice(src);
                })
                .is_ok(),
            None => false,
        };
        if !shm_grabbed {
            let image = self
                .conn
                .get_image(ImageFormat::Z_PIXMAP, self.root, x, y, width, height, !0)?
                .reply()
                .context("the X server sent no image")?;
            ensure!(image.data.len() >= len, "the X server sent a short image");
            pixels.copy_from_slice(&image.data[..len]);
        }
        if self.cursor {
            self.draw_cursor(pixels);
        }
        Ok(())
    }

    /// Blends the pointer's image (premultiplied ARGB) into the frame where it is.
    fn draw_cursor(&self, pixels: &mut [u8]) {
        let Some(cursor) = self
            .conn
            .xfixes_get_cursor_image()
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return;
        };
        let (rw, rh) = (i32::from(self.rect.width), i32::from(self.rect.height));
        let left = i32::from(cursor.x) - i32::from(cursor.xhot) - i32::from(self.rect.x);
        let top = i32::from(cursor.y) - i32::from(cursor.yhot) - i32::from(self.rect.y);
        let (cw, ch) = (i32::from(cursor.width), i32::from(cursor.height));
        for cy in 0..ch {
            let y = top + cy;
            if !(0..rh).contains(&y) {
                continue;
            }
            for cx in 0..cw {
                let x = left + cx;
                if !(0..rw).contains(&x) {
                    continue;
                }
                let argb = cursor.cursor_image[(cy * cw + cx) as usize];
                let a = argb >> 24;
                if a == 0 {
                    continue;
                }
                let at = ((y * rw + x) * 4) as usize;
                let inverse = 255 - a;
                for (channel, shift) in [(0, 0), (1, 8), (2, 16)] {
                    let source = (argb >> shift) & 0xff;
                    let dest = u32::from(pixels[at + channel]);
                    pixels[at + channel] = (source + dest * inverse / 255).min(255) as u8;
                }
            }
        }
    }
}

/// The RandR monitor the pointer is on.
fn monitor_under_pointer(conn: &RustConnection, root: u32) -> Option<Rect> {
    let pointer = conn.query_pointer(root).ok()?.reply().ok()?;
    let (px, py) = (i32::from(pointer.root_x), i32::from(pointer.root_y));
    let monitors = conn.randr_get_monitors(root, true).ok()?.reply().ok()?;
    monitors
        .monitors
        .iter()
        .find(|m| {
            (i32::from(m.x)..i32::from(m.x) + i32::from(m.width)).contains(&px)
                && (i32::from(m.y)..i32::from(m.y) + i32::from(m.height)).contains(&py)
        })
        .or_else(|| monitors.monitors.iter().find(|m| m.primary))
        .map(|m| Rect {
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        })
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn areas_are_clipped_to_the_screen_with_even_sides() {
        let screen = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let area = Rect {
            x: 1800,
            y: -10,
            width: 301,
            height: 101,
        };
        assert_eq!(
            area.clipped(screen),
            Some(Rect {
                x: 1800,
                y: 0,
                width: 120,
                height: 90
            })
        );
        let off = Rect {
            x: 3000,
            y: 0,
            width: 10,
            height: 10,
        };
        assert_eq!(off.clipped(screen), None);
    }
}
