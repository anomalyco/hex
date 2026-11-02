//! The X11 side of the Linux shortcuts: passive key and button grabs on the
//! root window, the cancel and Escape grabs held only while needed, the
//! keymap, and Settings' pointer grab for recording a mouse shortcut.

use super::*;

/// Temporary pointer grab while Settings records a shortcut. Keyboard input
/// stays with Rune; Escape, focus loss, saving and closing all drop the grab.
pub(crate) struct MouseCapture {
    pub(super) connection: RustConnection,
    pub(super) window: Window,
    pub(super) grabbed: bool,
    pub(super) modifiers: [u16; 4],
}

impl MouseCapture {
    pub(crate) fn start() -> Result<Self> {
        let (connection, root, keymap) = connect_x11()?;
        let window = connection.get_input_focus()?.reply()?.focus;
        let pid_atom = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        let mut ancestor = window;
        loop {
            if ancestor == root || ancestor <= 1 {
                return Err(eyre!("Hex must be focused to record a mouse shortcut"));
            }
            let property = connection
                .get_property(
                    false,
                    ancestor,
                    pid_atom,
                    x11rb::protocol::xproto::AtomEnum::CARDINAL,
                    0,
                    1,
                )?
                .reply()?;
            if let Some(pid) = property.value32().and_then(|mut values| values.next()) {
                if pid != std::process::id() {
                    return Err(eyre!("Hex must be focused to record a mouse shortcut"));
                }
                break;
            }
            ancestor = connection.query_tree(ancestor)?.reply()?.parent;
        }
        let mut modifiers = [0; 4];
        for (slot, symbols) in modifiers.iter_mut().zip([
            &[XK_CONTROL_L, XK_CONTROL_R][..],
            &[XK_ALT_L, XK_ALT_R][..],
            &[XK_SHIFT_L, XK_SHIFT_R][..],
            &[XK_SUPER_L, XK_SUPER_R][..],
        ]) {
            *slot = u16::from(keymap.modifier_for(&connection, symbols)?);
        }
        Ok(Self {
            connection,
            window,
            grabbed: false,
            modifiers,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<Option<LinuxHotkey>> {
        if self.connection.get_input_focus()?.reply()?.focus != self.window {
            return Err(eyre!("Shortcut recording lost focus"));
        }
        if !self.grabbed {
            // Wait for the click that opened Record to be released.
            if u16::from(self.connection.query_pointer(self.window)?.reply()?.mask) & 0x1f00 != 0 {
                return Ok(None);
            }
            let status = self
                .connection
                .grab_pointer(
                    false,
                    self.window,
                    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    x11rb::NONE,
                    x11rb::NONE,
                    x11rb::CURRENT_TIME,
                )?
                .reply()?
                .status;
            if status == x11rb::protocol::xproto::GrabStatus::ALREADY_GRABBED {
                return Ok(None);
            }
            if status != x11rb::protocol::xproto::GrabStatus::SUCCESS {
                return Err(eyre!("Could not record mouse input: {status:?}"));
            }
            self.grabbed = true;
        }
        while let Some(event) = self.connection.poll_for_event()? {
            if let Event::ButtonPress(event) = event {
                let size = self.connection.get_geometry(self.window)?.reply()?;
                if !event.same_screen
                    || event.event_x < 0
                    || event.event_y < 0
                    || event.event_x as u16 >= size.width
                    || event.event_y as u16 >= size.height
                {
                    return Err(eyre!("Shortcut recording clicked outside Hex"));
                }
                let Some(key) = mouse_name(event.detail) else {
                    continue;
                }; // wheel scrolling is not a button binding
                let state = u16::from(event.state);
                let [control, alt, shift, super_key] = self.modifiers.map(|mask| state & mask != 0);
                return Ok(Some(LinuxHotkey {
                    control,
                    alt,
                    shift,
                    super_key,
                    key,
                }));
            }
        }
        Ok(None)
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.connection.ungrab_pointer(x11rb::CURRENT_TIME);
            let _ = self.connection.flush();
        }
    }
}

pub(crate) fn connect_x11() -> Result<(RustConnection, Window, Keymap)> {
    let (connection, screen) =
        RustConnection::connect(None).wrap_err("could not connect to X11")?;
    let root = connection.setup().roots[screen].root;
    let keymap = Keymap::read(&connection)?;
    Ok((connection, root, keymap))
}

impl Backend {
    pub(super) fn run_x11(
        self,
        mut dispatcher: Dispatcher,
        binding: LinuxHotkey,
        options: HotkeyOptions,
    ) -> Result<()> {
        let (connection, root, keymap) = connect_x11()?;
        // A shortcut held while the previous monitor stopped never sends
        // its release here.
        dispatcher.keys.released_all();
        let mut grabs = X11Grabs {
            connection: &connection,
            root,
            num_lock: keymap
                .modifier_for(&connection, &[XK_NUM_LOCK])
                .unwrap_or(ModMask::M2),
            escape: keymap.keycode(XK_ESCAPE)?,
            keys: Vec::new(),
            escape_grabbed: Vec::new(),
            cancel: None,
            cancel_grabbed: Vec::new(),
        };
        let requested = [
            Some((Shortcut::Dictation, binding)),
            options.paste_last.map(|key| (Shortcut::PasteLast, key)),
            options.capture.map(|key| (Shortcut::Capture, key)),
            options
                .raw_dictation
                .map(|key| (Shortcut::RawDictation, key)),
        ];
        for (shortcut, key) in requested.into_iter().flatten() {
            let mouse = mouse_button(&key.key);
            let code = match mouse {
                Some(button) => button,
                None => keymap.keycode(keysym(&key.key)?)?,
            };
            let mask = key.modifier_mask(&connection, &keymap)?;
            grabs
                .grab(shortcut, code, mask, mouse.is_some())
                .wrap_err_with(|| {
                    format!("{} is already in use by another X11 client", key.label())
                })?;
        }
        // Toggle's cancel key: grabbed only while a recording runs.
        grabs.cancel = match options.cancel.as_ref().map(|key| -> Result<X11CancelKey> {
            if mouse_button(&key.key).is_some() {
                return Err(eyre!("a mouse button can't cancel on X11"));
            }
            Ok(X11CancelKey {
                code: keymap.keycode(keysym(&key.key)?)?,
                mask: key.modifier_mask(&connection, &keymap)?,
            })
        }) {
            Some(Ok(key)) => Some(key),
            Some(Err(error)) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "the cancel shortcut can't be grabbed; recordings can't be cancelled"
                );
                None
            }
            None => None,
        };
        let cancel_on_escape = grabs.cancel.is_some_and(|key| key.code == grabs.escape);
        connection.flush()?;
        self.mark_ready(DesktopShortcuts::Registered);

        let lock_bits = u16::from(ModMask::LOCK) | u16::from(grabs.num_lock);
        let mut keys = X11Keys {
            detectable_repeat: enable_detectable_auto_repeat(&connection),
            ..X11Keys::default()
        };
        let mut escape_down = false;
        let mut cancel_down = false;
        // Another client held Escape or the cancel key at the last try;
        // warn once per episode.
        let mut escape_taken = false;
        let mut cancel_taken = false;
        while !self.stopping() {
            let mut inputs = keys.expire(Instant::now());
            if let Some(event) = connection.poll_for_event()? {
                match event {
                    Event::KeyPress(event) if event.detail == grabs.escape => {
                        escape_down = true;
                        let mask = u16::from(event.state) & 0xff & !lock_bits;
                        // Drawing takes Escape first; it stops only that.
                        inputs.push(
                            if !annotate_drawing() && grabs.cancel_matches(event.detail, mask) {
                                Input::Cancel
                            } else {
                                Input::Escape
                            },
                        );
                    }
                    Event::KeyRelease(event) if event.detail == grabs.escape => {
                        escape_down = false;
                    }
                    Event::KeyPress(event)
                        if !grabs.cancel_grabbed.is_empty()
                            && grabs.cancel_matches(
                                event.detail,
                                u16::from(event.state) & 0xff & !lock_bits,
                            ) =>
                    {
                        cancel_down = true;
                        inputs.push(Input::Cancel);
                    }
                    Event::KeyPress(event) => {
                        let mask = u16::from(event.state) & 0xff & !lock_bits;
                        let shortcut = grabs
                            .keys
                            .iter()
                            .find(|grab| {
                                !grab.mouse && grab.code == event.detail && grab.mask == mask
                            })
                            .or_else(|| {
                                grabs.keys.iter().find(|grab| {
                                    !grab.mouse
                                        && grab.code == event.detail
                                        && keys.held(grab.shortcut)
                                })
                            })
                            .map(|grab| grab.shortcut);
                        inputs.extend(shortcut.and_then(|shortcut| keys.press(shortcut)));
                    }
                    Event::KeyRelease(event) => {
                        if grabs.cancel.is_some_and(|key| key.code == event.detail) {
                            cancel_down = false;
                        }
                        for grab in grabs
                            .keys
                            .iter()
                            .filter(|grab| !grab.mouse && grab.code == event.detail)
                        {
                            inputs.extend(keys.release(grab.shortcut, Instant::now()));
                        }
                    }
                    Event::ButtonPress(event) => {
                        let mask = u16::from(event.state) & 0xff & !lock_bits;
                        if let Some(grab) = grabs.keys.iter().find(|grab| {
                            grab.mouse && grab.code == event.detail && grab.mask == mask
                        }) {
                            inputs.extend(keys.press(grab.shortcut));
                        }
                    }
                    Event::ButtonRelease(event) => {
                        for grab in grabs
                            .keys
                            .iter()
                            .filter(|grab| grab.mouse && grab.code == event.detail)
                        {
                            // Buttons do not autorepeat: preserve the release boundary.
                            if keys.held[grab.shortcut.index()] {
                                keys.held[grab.shortcut.index()] = false;
                                inputs.push(Input::Released(grab.shortcut));
                            }
                        }
                    }
                    _ => {}
                }
            } else if inputs.is_empty() {
                thread::sleep(POLL_INTERVAL);
            }
            for input in inputs {
                // The X11 paste waits on held modifiers only while one of
                // Hex's shortcuts is down; see `wait_for_modifiers`.
                match input {
                    Input::Pressed(shortcut) => dispatcher.keys.pressed(shortcut),
                    Input::Released(shortcut) => dispatcher.keys.released(shortcut),
                    _ => {}
                }
                dispatcher.input(input);
            }
            dispatcher.commands();
            // Escape is Hex's only while Annotate draws or, as Toggle's
            // cancel key, while a recording runs, and until its release so
            // the release never reaches the app. Grabbing before the flush
            // holds the key before dictation hears that recording started.
            let wants_cancel = dispatcher.state.wants_cancel();
            let wants_escape =
                annotate_drawing() || escape_down || (wants_cancel && cancel_on_escape);
            if wants_escape && grabs.escape_grabbed.is_empty() {
                let available = grabs.grab_escape()?;
                if !available && !escape_taken {
                    tracing::warn!("another application holds Escape");
                }
                escape_taken = !available;
            } else if !wants_escape && !grabs.escape_grabbed.is_empty() {
                grabs.release_escape()?;
            }
            if !cancel_on_escape {
                let wants_cancel = wants_cancel || cancel_down;
                if wants_cancel && grabs.cancel_grabbed.is_empty() {
                    let available = grabs.grab_cancel()?;
                    if !available && !cancel_taken {
                        tracing::warn!("another application holds the cancel shortcut");
                    }
                    cancel_taken = !available;
                } else if !wants_cancel && !grabs.cancel_grabbed.is_empty() {
                    grabs.release_cancel()?;
                }
            }
            if !dispatcher.flush()? {
                return Ok(());
            }
        }
        Ok(())
    }
}

/// Asks the X server to report a held key's auto-repeat as repeated presses
/// with one release at the end (XKB detectable auto-repeat), so a release is
/// always physical. Returns whether the server granted it.
pub(super) fn enable_detectable_auto_repeat(connection: &RustConnection) -> bool {
    use x11rb::protocol::xkb::{self, ConnectionExt as _};
    const CORE_KEYBOARD: xkb::DeviceSpec = 0x0100;
    let result = (|| -> Result<bool> {
        if !connection.xkb_use_extension(1, 0)?.reply()?.supported {
            return Ok(false);
        }
        let flag = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
        let reply = connection
            .xkb_per_client_flags(
                CORE_KEYBOARD,
                flag,
                flag,
                xkb::BoolCtrl::from(0u32),
                xkb::BoolCtrl::from(0u32),
                xkb::BoolCtrl::from(0u32),
            )?
            .reply()?;
        Ok(u32::from(reply.value) & u32::from(flag) != 0)
    })();
    match result {
        Ok(granted) => granted,
        Err(error) => {
            tracing::debug!(
                error = format!("{error:#}"),
                "no XKB detectable auto-repeat"
            );
            false
        }
    }
}

/// Turns X11 key events into presses and releases. With detectable
/// auto-repeat a held key repeats only its press, so a release counts at
/// once. Without it, auto-repeat sends a release before every repeated
/// press, so a release counts only once [`RELEASE_GRACE`] passes without a
/// new press.
#[derive(Default)]
pub(super) struct X11Keys {
    pub(super) held: [bool; Shortcut::ALL.len()],
    pub(super) releasing: [Option<Instant>; Shortcut::ALL.len()],
    pub(super) detectable_repeat: bool,
}

impl X11Keys {
    pub(super) fn held(&self, shortcut: Shortcut) -> bool {
        self.held[shortcut.index()]
    }

    pub(super) fn press(&mut self, shortcut: Shortcut) -> Option<Input> {
        let index = shortcut.index();
        if self.releasing[index].take().is_some() || self.held[index] {
            return None;
        }
        self.held[index] = true;
        Some(Input::Pressed(shortcut))
    }

    pub(super) fn release(&mut self, shortcut: Shortcut, now: Instant) -> Option<Input> {
        let index = shortcut.index();
        if !self.held[index] {
            return None;
        }
        if self.detectable_repeat {
            self.held[index] = false;
            self.releasing[index] = None;
            return Some(Input::Released(shortcut));
        }
        self.releasing[index] = Some(now);
        None
    }

    pub(super) fn expire(&mut self, now: Instant) -> Vec<Input> {
        let mut released = Vec::new();
        for shortcut in Shortcut::ALL {
            let index = shortcut.index();
            if self.releasing[index]
                .is_some_and(|at| now.saturating_duration_since(at) >= RELEASE_GRACE)
            {
                self.releasing[index] = None;
                self.held[index] = false;
                released.push(Input::Released(shortcut));
            }
        }
        released
    }
}

pub(super) struct X11Grab {
    pub(super) mouse: bool,
    pub(super) shortcut: Shortcut,
    pub(super) code: u8,
    pub(super) mask: u16,
    pub(super) variants: [ModMask; 4],
}

/// Toggle's cancel key on X11.
#[derive(Clone, Copy)]
pub(super) struct X11CancelKey {
    pub(super) code: u8,
    pub(super) mask: ModMask,
}

/// Passive grabs on the root window, released on drop.
pub(super) struct X11Grabs<'a> {
    pub(super) connection: &'a RustConnection,
    pub(super) root: Window,
    pub(super) num_lock: ModMask,
    pub(super) escape: u8,
    pub(super) keys: Vec<X11Grab>,
    pub(super) escape_grabbed: Vec<ModMask>,
    pub(super) cancel: Option<X11CancelKey>,
    pub(super) cancel_grabbed: Vec<ModMask>,
}

impl X11Grabs<'_> {
    pub(super) fn grab(
        &mut self,
        shortcut: Shortcut,
        code: u8,
        mask: ModMask,
        mouse: bool,
    ) -> Result<(), ReplyError> {
        let variants = lock_variants(mask, self.num_lock);
        if mouse {
            for (index, modifiers) in variants.iter().copied().enumerate() {
                let result = self
                    .connection
                    .grab_button(
                        false,
                        self.root,
                        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        x11rb::NONE,
                        x11rb::NONE,
                        ButtonIndex::from(code),
                        modifiers,
                    )?
                    .check();
                if let Err(error) = result {
                    for previous in &variants[..index] {
                        self.connection
                            .ungrab_button(ButtonIndex::from(code), self.root, *previous)?
                            .check()?;
                    }
                    return Err(error);
                }
            }
        } else {
            grab_variants(self.connection, self.root, code, &variants)?;
        }
        self.keys.push(X11Grab {
            mouse,
            shortcut,
            code,
            mask: u16::from(mask),
            variants,
        });
        Ok(())
    }

    /// Returns false when another client holds Escape.
    pub(super) fn grab_escape(&mut self) -> Result<bool> {
        let bare = lock_variants(ModMask::default(), self.num_lock);
        self.escape_grabbed = grab_escape(self.connection, self.root, self.escape, &bare)?;
        self.connection.flush()?;
        Ok(!self.escape_grabbed.is_empty())
    }

    pub(super) fn release_escape(&mut self) -> Result<()> {
        for modifiers in self.escape_grabbed.drain(..) {
            self.connection
                .ungrab_key(self.escape, self.root, modifiers)?
                .check()?;
        }
        self.connection.flush()?;
        Ok(())
    }

    /// Whether a press with `mask` (lock bits cleared) is the cancel key.
    pub(super) fn cancel_matches(&self, code: u8, mask: u16) -> bool {
        self.cancel
            .is_some_and(|key| key.code == code && u16::from(key.mask) == mask)
    }

    /// Returns false when another client holds the cancel key.
    pub(super) fn grab_cancel(&mut self) -> Result<bool> {
        let Some(key) = self.cancel else {
            return Ok(false);
        };
        let variants = lock_variants(key.mask, self.num_lock);
        match grab_variants(self.connection, self.root, key.code, &variants) {
            Ok(()) => self.cancel_grabbed = variants.to_vec(),
            Err(error) if is_grab_conflict(&error) => {}
            Err(error) => return Err(error).wrap_err("could not grab the cancel key"),
        }
        self.connection.flush()?;
        Ok(!self.cancel_grabbed.is_empty())
    }

    pub(super) fn release_cancel(&mut self) -> Result<()> {
        let Some(key) = self.cancel else {
            return Ok(());
        };
        for modifiers in self.cancel_grabbed.drain(..) {
            self.connection
                .ungrab_key(key.code, self.root, modifiers)?
                .check()?;
        }
        self.connection.flush()?;
        Ok(())
    }
}

impl Drop for X11Grabs<'_> {
    fn drop(&mut self) {
        for &modifiers in &self.escape_grabbed {
            let _ = self
                .connection
                .ungrab_key(self.escape, self.root, modifiers);
        }
        if let Some(key) = self.cancel {
            for &modifiers in &self.cancel_grabbed {
                let _ = self.connection.ungrab_key(key.code, self.root, modifiers);
            }
        }
        for grab in &self.keys {
            for modifiers in grab.variants {
                if grab.mouse {
                    let _ = self.connection.ungrab_button(
                        ButtonIndex::from(grab.code),
                        self.root,
                        modifiers,
                    );
                } else {
                    let _ = self.connection.ungrab_key(grab.code, self.root, modifiers);
                }
            }
        }
        let _ = self.connection.flush();
    }
}

/// True when the server refused a passive grab because another client holds it.
/// `GrabKey` with a conflicting grab fails with `BadAccess` (`Access`); any
/// other error (connection loss, bad value) must stay fatal.
pub(super) fn is_grab_conflict(error: &ReplyError) -> bool {
    matches!(
        error,
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Access
    )
}

pub(super) fn grab_escape(
    connection: &RustConnection,
    root: Window,
    escape: u8,
    bare_modifiers: &[ModMask],
) -> Result<Vec<ModMask>> {
    match grab_variants(connection, root, escape, &[ModMask::ANY]) {
        Ok(()) => Ok(vec![ModMask::ANY]),
        Err(error) if is_grab_conflict(&error) => {
            // A modified Escape shortcut must not prevent grabbing plain Escape.
            match grab_variants(connection, root, escape, bare_modifiers) {
                Ok(()) => Ok(bare_modifiers.to_vec()),
                Err(error) if is_grab_conflict(&error) => Ok(Vec::new()),
                Err(error) => Err(error).wrap_err("could not grab the Escape key"),
            }
        }
        Err(error) => Err(error).wrap_err("could not grab the Escape key"),
    }
}

pub(super) fn keysym(key: &str) -> Result<u32> {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "space" => Ok(XK_SPACE),
        "escape" => Ok(XK_ESCAPE),
        "enter" | "return" => Ok(0xff0d),
        "tab" => Ok(0xff09),
        "backspace" => Ok(0xff08),
        "menu" => Ok(0xff67),
        "pause" => Ok(0xff13),
        "scroll lock" | "scrolllock" | "scroll_lock" => Ok(0xff14),
        // XK_section, the ISO key left of 1.
        "§" | "section" => Ok(0xa7),
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() => {
            Ok(u32::from(key.as_bytes()[0]))
        }
        key if key.starts_with('f') => key[1..]
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0xffbd + number)
            .ok_or_else(|| eyre!("unsupported X11 function key: {key}")),
        _ => Err(eyre!("unsupported X11 hotkey key: {key}")),
    }
}

pub(super) fn grab_variants(
    connection: &RustConnection,
    root: Window,
    key: u8,
    modifiers: &[ModMask],
) -> Result<(), ReplyError> {
    let mut grabbed = Vec::new();
    for &modifiers in modifiers {
        let result = connection
            .grab_key(
                false,
                root,
                modifiers,
                key,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        if let Err(error) = result {
            // Wait for each release: another client may grab the same key
            // right after this returns, and must not race the ungrab.
            for modifiers in grabbed {
                if let Ok(cookie) = connection.ungrab_key(key, root, modifiers) {
                    let _ = cookie.check();
                }
            }
            return Err(error);
        }
        grabbed.push(modifiers);
    }
    Ok(())
}

pub(super) fn lock_variants(base: ModMask, num_lock: ModMask) -> [ModMask; 4] {
    [
        base,
        base | ModMask::LOCK,
        base | num_lock,
        base | ModMask::LOCK | num_lock,
    ]
}

pub(crate) struct Keymap {
    pub(super) min: u8,
    pub(super) symbols_per_keycode: usize,
    pub(super) symbols: Vec<u32>,
}

impl Keymap {
    pub(crate) fn read(connection: &RustConnection) -> Result<Self> {
        let setup = connection.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode - min + 1;
        let mapping = connection.get_keyboard_mapping(min, count)?.reply()?;
        Ok(Self {
            min,
            symbols_per_keycode: usize::from(mapping.keysyms_per_keycode),
            symbols: mapping.keysyms,
        })
    }

    pub(crate) fn keycode(&self, keysym: u32) -> Result<u8> {
        self.symbols
            .chunks(self.symbols_per_keycode)
            .position(|symbols| symbols.contains(&keysym))
            .map(|index| self.min + index as u8)
            .ok_or_else(|| eyre!("active X11 keymap has no key for keysym {keysym:#x}"))
    }

    pub(crate) fn modifier_for(
        &self,
        connection: &RustConnection,
        keysyms: &[u32],
    ) -> Result<ModMask> {
        let keycodes = keysyms
            .iter()
            .filter_map(|keysym| self.keycode(*keysym).ok())
            .collect::<Vec<_>>();
        let mapping = connection.get_modifier_mapping()?.reply()?;
        let width = usize::from(mapping.keycodes_per_modifier());
        mapping
            .keycodes
            .chunks(width)
            .position(|codes| codes.iter().any(|code| keycodes.contains(code)))
            .map(|index| ModMask::from(1_u16 << index))
            .ok_or_else(|| eyre!("active X11 keymap does not map the requested modifier"))
    }
}
