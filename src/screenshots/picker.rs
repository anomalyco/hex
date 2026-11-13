//! The permission reply and recording intent belong to one picker opening.

#[derive(Default)]
pub(super) struct Picker {
    next: u64,
    pub(super) active: Option<Pick>,
}

pub(super) struct Pick {
    pub(super) id: u64,
    pub(super) record: bool,
    pub(super) window: bool,
    pub(super) ready: bool,
}

impl Picker {
    pub(super) const fn new() -> Self {
        Self {
            next: 0,
            active: None,
        }
    }

    pub(super) fn begin(&mut self, record: bool) -> Option<u64> {
        if self.active.is_some() {
            return None;
        }
        self.next += 1;
        let id = self.next;
        self.active = Some(Pick {
            id,
            record,
            window: false,
            ready: false,
        });
        Some(id)
    }

    pub(super) fn current(&self, id: u64) -> bool {
        self.active.as_ref().is_some_and(|pick| pick.id == id)
    }

    pub(super) fn permission(&mut self, id: u64, allowed: bool) -> bool {
        if !self.current(id) {
            return false;
        }
        if allowed {
            self.active.as_mut().unwrap().ready = true;
        } else {
            self.active = None;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_permission_never_reopens_or_changes_a_new_pick() {
        let mut picker = Picker::new();
        let old = picker.begin(true).unwrap();
        picker.active = None;
        assert!(!picker.permission(old, true));
        let new = picker.begin(false).unwrap();
        assert!(!picker.permission(old, false));
        assert!(!picker.permission(old, true));
        assert!(picker.current(new));
        assert!(!picker.active.as_ref().unwrap().record);
        assert!(!picker.active.as_ref().unwrap().ready);
        assert!(picker.permission(new, true));
        assert!(picker.active.as_ref().unwrap().ready);
    }
}
