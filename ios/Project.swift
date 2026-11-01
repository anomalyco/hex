import Keel

// MARK: - Packages

let fluidAudio = PackageRef(
    "FluidAudio",
    url: "https://github.com/FluidInference/FluidAudio.git",
    .exact("0.15.5")
)

// MARK: - Targets

let hexKeyboard = ExtensionTarget(
    "HEXKeyboard",
    kind: .keyboard,
    platform: .iOS("18.0"),
    bundleID: "com.kitlangton.hex.mobile.keyboard",
    sources: [
        .folder("HEXKeyboard"),
        .folder("HEXShared"),
    ],
    infoPlist: .file("HEXKeyboard/Info.plist"),
    entitlements: .file("HEXKeyboard/HEXKeyboard.entitlements"),
    settings: .signing(team: "QC99C9JE59")
        <> .version(marketing: "0.2.0", build: 1)
        <> .set("APPLICATION_EXTENSION_API_ONLY", true)
        <> .set("PRODUCT_NAME", "HEXKeyboard")
)

let hexWidgets = ExtensionTarget(
    "HEXWidgets",
    kind: .widgetKit,
    platform: .iOS("18.0"),
    bundleID: "com.kitlangton.hex.mobile.widgets",
    sources: [
        .folder("HEXWidgets"),
        .folder("HEXActivity"),
        .file("HEXShared/HexTheme.swift", phase: .sources),
    ],
    infoPlist: .file("HEXWidgets/Info.plist"),
    settings: .signing(team: "QC99C9JE59")
        <> .version(marketing: "0.2.0", build: 1)
        <> .set("APPLICATION_EXTENSION_API_ONLY", true)
        <> .set("PRODUCT_NAME", "HEXWidgets")
)

let hexMobile = AppTarget(
    "HEXMobile",
    platform: .iOS("18.0"),
    bundleID: "com.kitlangton.hex.mobile",
    sources: [
        .folder("HEXMobile"),
        .folder("HEXShared"),
        .folder("HEXActivity"),
        .resources("../app/AppIcon.icon"),
    ],
    packages: [
        fluidAudio.product(),
    ],
    embed: [hexKeyboard, hexWidgets],
    infoPlist: .file("HEXMobile/Info.plist"),
    entitlements: .file("HEXMobile/HEXMobile.entitlements"),
    settings: .signing(team: "QC99C9JE59")
        <> .version(marketing: "0.2.0", build: 1)
        <> .appIcon("AppIcon")
        <> .set("ASSETCATALOG_COMPILER_ACCENT_COLOR_NAME", "AccentColor")
        <> .set("PRODUCT_NAME", "HEX")
)

let hexMobileTests = UnitTestTarget(
    "HEXMobileTests",
    testing: hexMobile,
    bundleID: "com.kitlangton.hex.mobile.tests",
    sources: [
        .folder("HEXMobileTests"),
        .resources("../tests/fixtures/vocabulary/audio/term-hex-1.wav"),
    ],
    settings: .signing(team: "QC99C9JE59")
)

keelExport(
    Project(
        "HEXMobile",
        bundleIDPrefix: "com.kitlangton.hex",
        defaultPlatform: .iOS("18.0"),
        projectSettings: .swift(.v6) <> .signing(team: "QC99C9JE59"),
        targets: [hexMobile, hexKeyboard, hexWidgets, hexMobileTests],
        schemes: [
            Scheme("HEXMobile", build: [hexMobile], launch: hexMobile, unitTests: [hexMobileTests]),
        ]
    )
)
