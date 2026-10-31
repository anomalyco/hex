//! Line icons that lead the settings rows, drawn on a 24 point grid like
//! the theme's own.

use rune::Icon;

macro_rules! line_icon {
    ($static:ident, $name:literal, [$($d:literal),+ $(,)?]) => {
        pub(super) static $static: Icon = Icon {
            name: $name,
            view: 24.0,
            body: concat!($(
                "<path d=\"", $d,
                "\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.75\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>"
            ),+),
        };
    };
}

line_icon!(
    KEYBOARD,
    "hex-row-keyboard",
    [
        "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
        "M6 8h.01",
        "M10 8h.01",
        "M14 8h.01",
        "M18 8h.01",
        "M8 12h.01",
        "M12 12h.01",
        "M16 12h.01",
        "M7 16h10",
    ]
);
line_icon!(
    BLUETOOTH,
    "hex-row-bluetooth",
    ["m7 7 10 10-5 5V2l5 5L7 17"]
);
line_icon!(
    WAVE,
    "hex-row-wave",
    [
        "M2 10v3", "M6 6v11", "M10 3v18", "M14 8v7", "M18 5v13", "M22 10v3",
    ]
);
line_icon!(
    LANGUAGES,
    "hex-row-languages",
    [
        "m5 8 6 6",
        "m4 14 6-6 2-3",
        "M2 5h12",
        "M7 2h1",
        "m22 22-5-10-5 10",
        "M14 18h6",
    ]
);
line_icon!(
    MIC,
    "hex-row-mic",
    [
        "M12 2a3 3 0 0 1 3 3v7a3 3 0 0 1-6 0V5a3 3 0 0 1 3-3z",
        "M19 10v2a7 7 0 0 1-14 0v-2",
        "M12 19v3",
    ]
);
line_icon!(
    POWER,
    "hex-row-power",
    ["M12 2v10", "M18.4 6.6a9 9 0 1 1-12.77.04"]
);
line_icon!(
    SIDE,
    "hex-row-side",
    ["M8 3 4 7l4 4", "M4 7h16", "m16 21 4-4-4-4", "M20 17H4"]
);
line_icon!(
    HAND,
    "hex-row-hand",
    [
        "M18 11V6a2 2 0 0 0-4 0",
        "M14 10V4a2 2 0 0 0-4 0v2",
        "M10 10.5V6a2 2 0 0 0-4 0v8",
        "M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15",
    ]
);
line_icon!(
    LOCK,
    "hex-row-lock",
    [
        "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z",
        "M7 11V7a5 5 0 0 1 10 0v4",
    ]
);
line_icon!(
    TIMER,
    "hex-row-timer",
    [
        "M10 2h4",
        "m12 14 3-3",
        "M4 14a8 8 0 1 0 16 0a8 8 0 1 0-16 0",
    ]
);
line_icon!(
    ZAP,
    "hex-row-zap",
    [
        "M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z",
    ]
);
line_icon!(
    BOOK,
    "hex-row-book",
    [
        "M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20",
        "m8 13 4-7 4 7",
        "M9.1 11h5.7",
    ]
);
line_icon!(
    HINTS,
    "hex-row-hints",
    ["M4 7V4h16v3", "M9 20h6", "M12 4v16"]
);
line_icon!(
    PASTE,
    "hex-row-paste",
    [
        "M9 2h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z",
        "M16 4h2a2 2 0 0 1 2 2v1.344",
        "M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 1.793-1.113",
        "M11 14h10",
        "m17 18 4-4-4-4",
    ]
);
line_icon!(
    MUTE,
    "hex-row-mute",
    [
        "M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z",
        "M22 9l-6 6",
        "M16 9l6 6",
    ]
);
line_icon!(
    VOLUME,
    "hex-row-volume",
    [
        "M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z",
        "M16 9a5 5 0 0 1 0 6",
        "M19.364 18.364a9 9 0 0 0 0-12.728",
    ]
);
line_icon!(
    PANEL,
    "hex-row-panel",
    [
        "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
        "M3 9h18",
    ]
);
line_icon!(
    DOCK,
    "hex-row-dock",
    [
        "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
        "M3 15h18",
    ]
);
line_icon!(TERMINAL, "hex-row-terminal", ["m4 17 6-6-6-6", "M12 19h8"]);
line_icon!(
    PLAY,
    "hex-row-play",
    ["M6 4.5v15a1 1 0 0 0 1.5.86l12.5-7.5a1 1 0 0 0 0-1.72L7.5 3.64A1 1 0 0 0 6 4.5z"]
);
line_icon!(
    BELL,
    "hex-row-bell",
    [
        "M10.268 21a2 2 0 0 0 3.464 0",
        "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326",
    ]
);
line_icon!(
    MUSIC,
    "hex-row-music",
    [
        "M9 18V5l12-2v13",
        "M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
        "M15 16a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    ]
);
line_icon!(
    GLOBE,
    "hex-row-globe",
    [
        "M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0",
        "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20",
        "M2 12h20",
    ]
);
line_icon!(
    REFRESH,
    "hex-row-refresh",
    [
        "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8",
        "M21 3v5h-5",
        "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16",
        "M8 16H3v5",
    ]
);
line_icon!(
    FOLDER,
    "hex-row-folder",
    [
        "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z",
    ]
);
line_icon!(
    SPARKLES,
    "hex-row-sparkles",
    [
        "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z",
    ]
);
line_icon!(
    SPEAKER,
    "hex-row-speaker",
    [
        "M6 2h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z",
        "M12 6h.01",
        "M9 14a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    ]
);
line_icon!(
    PILL,
    "hex-row-pill",
    ["M8 7h8a5 5 0 0 1 0 10H8A5 5 0 0 1 8 7z", "M9 12h.01",]
);
line_icon!(
    CALENDAR,
    "hex-row-calendar",
    [
        "M8 2v4",
        "M16 2v4",
        "M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
        "M3 10h18",
    ]
);
line_icon!(
    CLOCK,
    "hex-row-clock",
    ["M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0", "M12 6v6l4 2"]
);
line_icon!(
    MOUSE,
    "hex-row-mouse",
    [
        "M12 2a6 6 0 0 1 6 6v8a6 6 0 0 1-12 0V8a6 6 0 0 1 6-6z",
        "M12 6v4"
    ]
);
