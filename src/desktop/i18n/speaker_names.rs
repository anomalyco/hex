//! Labs › Speaker names' strings: the card that turns the lab on. Kept
//! beside the main table so it stays under the source cap, like
//! `meeting_mute`.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Speaker names", ["Imiona rozmówców", "发言人姓名", "話者名", "Sprechernamen", "Nombres de quienes hablan", "Імена мовців"]),
    ("Read who is speaking from Slack huddles to name remote speakers in meeting transcripts", ["Odczytuje z huddle w Slacku, kto mówi, i podpisuje zdalnych rozmówców w transkrypcjach spotkań", "从 Slack 语音小组会读取谁在发言，并在会议转写中标出远端发言人的姓名", "Slack ハドルで誰が話しているかを読み取り、会議の文字起こしでリモートの話者に名前を付けます", "Liest aus Slack-Huddles, wer gerade spricht, und benennt entfernte Sprecher in Meeting-Transkripten", "Lee quién habla en los huddles de Slack para poner nombre a los interlocutores remotos en las transcripciones de reuniones", "Зчитує з хадлів у Slack, хто говорить, і називає віддалених мовців у транскриптах зустрічей"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Speaker names", "Noms des intervenants"),
    (
        "Read who is speaking from Slack huddles to name remote speakers in meeting transcripts",
        "Lit qui parle dans les huddles Slack pour nommer les intervenants distants dans les transcriptions de réunion",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Speaker names", "Nomes de quem fala"),
    (
        "Read who is speaking from Slack huddles to name remote speakers in meeting transcripts",
        "Lê quem está falando nos huddles do Slack para dar nome aos participantes remotos nas transcrições de reunião",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Speaker names", "Имена говорящих"),
    (
        "Read who is speaking from Slack huddles to name remote speakers in meeting transcripts",
        "Считывает из созвонов в Slack, кто говорит, и подписывает удалённых собеседников в расшифровках встреч",
    ),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Speaker names", "Beszélők neve"),
    (
        "Read who is speaking from Slack huddles to name remote speakers in meeting transcripts",
        "A Slack-hívásokból kiolvassa, ki beszél, és névvel jelöli a távoli beszélőket a megbeszélések átirataiban",
    ),
];
