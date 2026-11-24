//! The area each Labs feature belongs to: Settings › Labs lists them under
//! one collapsible header per group.

/// One area of Settings › Labs. Every feature picks one (`Feature::group`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Dictation,
    /// Paste, formatting, and spoken punctuation.
    Output,
    History,
    Meetings,
    Annotate,
    Shortcuts,
    Models,
}

impl Group {
    /// In the order Labs lists them.
    pub const ALL: [Self; 7] = [
        Self::Dictation,
        Self::Output,
        Self::History,
        Self::Meetings,
        Self::Annotate,
        Self::Shortcuts,
        Self::Models,
    ];

    /// Its stable id: `hex://labs?category=<id>` and `labs.groups` in
    /// settings.json.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Output => "output",
            Self::History => "history",
            Self::Meetings => "meetings",
            Self::Annotate => "annotate",
            Self::Shortcuts => "shortcuts",
            Self::Models => "models",
        }
    }

    /// Its English header in Settings › Labs.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dictation => "Dictation",
            Self::Output => "Output",
            Self::History => "History",
            Self::Meetings => "Meetings",
            Self::Annotate => "Annotate",
            Self::Shortcuts => "Shortcuts",
            Self::Models => "Models",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Feature;
    use super::*;

    #[test]
    fn every_group_has_a_feature_and_round_trips_its_id() {
        for group in Group::ALL {
            assert_eq!(Group::from_id(group.id()), Some(group));
            assert!(
                Feature::ALL.iter().any(|feature| feature.group() == group),
                "{} holds no feature",
                group.id()
            );
        }
    }
}
