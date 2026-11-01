import Keel

// MARK: - Packages

let hexMacPackage = PackageRef("HexMacPackage", path: "HexMacPackage")

// MARK: - Targets

let hex = AppTarget(
    "Hex",
    platform: .macOS("15.0"),
    bundleID: "com.kitlangton.hex2",
    sources: [
        .folder("HexMacApp"),
        .resources("../app/AppIcon.icon"),
    ],
    packages: [
        hexMacPackage.product("HexMacFeature"),
    ],
    infoPlist: .file("HexMacApp/Info.plist"),
    entitlements: .file("HexMacApp/Hex.entitlements"),
    settings: .signing(team: "QC99C9JE59")
        <> .version(marketing: "2.2.0", build: 20200)
        <> .appIcon("AppIcon")
        <> .set("EXECUTABLE_NAME", "hex")
        <> .set("LD_RUNPATH_SEARCH_PATHS", [
            "$(inherited)",
            "@executable_path/../Frameworks",
            "/usr/lib/swift",
        ])
        <> .set("PRODUCT_NAME", "Hex"),
    postBuildScripts: [
        BuildScript(
            "Embed Sparkle & Moonshine Runtime Libraries",
            script: #"""
            set -eu
            ROOT_DIR="${SRCROOT}/.."
            FRAMEWORKS_DIR="${TARGET_BUILD_DIR}/${FRAMEWORKS_FOLDER_PATH}"
            RESOURCES_DIR="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}"
            mkdir -p "${FRAMEWORKS_DIR}" "${RESOURCES_DIR}"

            SPARKLE_FOUND=0
            for SPARKLE_CANDIDATE in "${ROOT_DIR}"/target/sparkle-*/Sparkle.framework /Users/kit/code/open-source/hex/target/sparkle-*/Sparkle.framework /Applications/Hex.app/Contents/Frameworks/Sparkle.framework; do
              if [ -d "${SPARKLE_CANDIDATE}" ]; then
                rm -rf "${FRAMEWORKS_DIR}/Sparkle.framework"
                /usr/bin/ditto "${SPARKLE_CANDIDATE}" "${FRAMEWORKS_DIR}/Sparkle.framework"
                SPARKLE_FOUND=1
                break
              fi
            done
            if [ "${SPARKLE_FOUND}" -eq 0 ] && [ -x "${ROOT_DIR}/scripts/setup-sparkle.sh" ]; then
              SPARKLE_DIR=$("${ROOT_DIR}/scripts/setup-sparkle.sh")
              /usr/bin/ditto "${SPARKLE_DIR}/Sparkle.framework" "${FRAMEWORKS_DIR}/Sparkle.framework"
            fi

            MOONSHINE_FOUND=0
            for MOONSHINE_CANDIDATE in "${ROOT_DIR}"/.venv/lib/python*/site-packages/moonshine_voice /Users/kit/code/open-source/hex/.venv/lib/python*/site-packages/moonshine_voice /Applications/Hex.app/Contents/Frameworks; do
              if [ -f "${MOONSHINE_CANDIDATE}/libmoonshine.dylib" ] && [ -f "${MOONSHINE_CANDIDATE}/libonnxruntime.1.23.2.dylib" ]; then
                cp -f "${MOONSHINE_CANDIDATE}/libmoonshine.dylib" "${FRAMEWORKS_DIR}/"
                cp -f "${MOONSHINE_CANDIDATE}/libonnxruntime.1.23.2.dylib" "${FRAMEWORKS_DIR}/"
                MOONSHINE_FOUND=1
                break
              fi
            done
            if [ "${MOONSHINE_FOUND}" -eq 0 ]; then
              echo "error: libmoonshine.dylib / libonnxruntime.1.23.2.dylib not found; run ./scripts/setup.sh" >&2
              exit 1
            fi

            if [ -d "${ROOT_DIR}/sdk/commands/dist" ]; then
              mkdir -p "${RESOURCES_DIR}/commands-sdk"
              cp -f "${ROOT_DIR}/sdk/commands/package.json" "${RESOURCES_DIR}/commands-sdk/"
              /usr/bin/ditto "${ROOT_DIR}/sdk/commands/dist" "${RESOURCES_DIR}/commands-sdk/dist"
              /usr/bin/ditto "${ROOT_DIR}/sdk/commands/workspace-template" "${RESOURCES_DIR}/commands-sdk/workspace-template"
            fi

            cp -f "${ROOT_DIR}/LICENSE" "${ROOT_DIR}/THIRD_PARTY_NOTICES.md" "${RESOURCES_DIR}/" || true
            """#
        ),
    ]
)

keelExport(
    Project(
        "Hex",
        bundleIDPrefix: "com.kitlangton",
        defaultPlatform: .macOS("15.0"),
        projectSettings: .swift(.v6)
            <> .signing(team: "QC99C9JE59")
            <> .set("ARCHS", "arm64")
            <> .set("ONLY_ACTIVE_ARCH", true),
        targets: [hex],
        schemes: [
            Scheme("Hex", build: [hex], launch: hex),
        ]
    )
)
