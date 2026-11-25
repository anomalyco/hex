import SwiftUI
import UIKit

/// One release's What's new: the App Store version that brings it, and its items.
struct WhatsNewRelease: Identifiable, Equatable {
    let version: String
    let items: [WhatsNewItem]

    var id: String { version }
}

/// A picture from the real app (Assets.xcassets › WhatsNew › `picture`), a name and a sentence.
struct WhatsNewItem: Identifiable, Equatable {
    let picture: String
    let title: LocalizedStringResource
    let detail: LocalizedStringResource
    /// What the picture shows, for VoiceOver.
    let pictureLabel: LocalizedStringResource

    var id: String { picture }
    var imageName: String { "WhatsNew/\(picture)" }

    static func == (lhs: WhatsNewItem, rhs: WhatsNewItem) -> Bool { lhs.picture == rhs.picture }
}

/// What else is going on when What's new could show.
struct WhatsNewMoment: Equatable {
    /// A link opened Hex since it last came to the front: the keyboard's hand-off or Open Hex,
    /// a words file, setup.
    var openedByLink = false
    var shownThisLaunch = false
    var setupSheet = false
    /// A Settings page over Home.
    var settingsPage = false
    /// Anything presented over Home: a words file's review, Safari, Send feedback.
    var presenting = false
    /// Recording, transcribing, or asking for the microphone.
    var dictationBusy = false
    var active = true
}

/// What's new, shown once in a sheet over Home on the first plain open after an update that
/// brings a list; never on a first run (onboarding is), never from a link, and never again
/// once it has shown. Home's What's new row opens it any time.
@MainActor
enum WhatsNew {
    /// Newest first. A release with something to show adds its list at the top, with a
    /// picture per item in Assets.xcassets › WhatsNew (docs/settings-and-home.md says how
    /// they're made).
    static let releases: [WhatsNewRelease] = [
        WhatsNewRelease(version: "1.0.5", items: [
            WhatsNewItem(
                picture: "languages",
                title: "Dutch, Swedish and Slovak",
                detail: "Add one in Keyboard › Languages to type with its own letters, like å, ä and ö.",
                pictureLabel: "Hex's Swedish keyboard, with the keys å, ä and ö."
            ),
            WhatsNewItem(
                picture: "emoji",
                title: "Emoji as you type",
                detail: "Type a word like “smile” and tap the 😊 above the keys to put it in its place.",
                pictureLabel: "The keyboard offering 😊 above the keys while “smile” is typed."
            ),
            WhatsNewItem(
                picture: "replacements",
                title: "Word replacements",
                detail: "In Dictation › Custom vocabulary, tell Hex to write “GitHub” when it hears “jet hub”, and share your list as a file.",
                pictureLabel: "Word replacements, with “jet hub” written as “GitHub”."
            ),
            WhatsNewItem(
                picture: "kept",
                title: "Kept when the screen locks",
                detail: "If your iPhone locks mid-dictation, what you said is kept: tap Paste in the keyboard to put it in.",
                pictureLabel: "The keyboard saying the dictation stopped at 0:44, with Paste."
            ),
            WhatsNewItem(
                picture: "key-colors",
                title: "Key colors",
                detail: "Turn on Key colors in Labs, which used to be Experiments, and pick a color for the keys.",
                pictureLabel: "Hex's keyboard with burgundy keys."
            ),
            WhatsNewItem(
                picture: "feedback",
                title: "Send feedback",
                detail: "Tap Send feedback at the bottom of Home to tell us what to fix or add, with a screenshot if it helps.",
                pictureLabel: "The Send feedback form."
            ),
        ]),
    ]

    /// The version of the last list that showed, in Hex's own `UserDefaults`.
    static let seenKey = "whatsNew.seen"

    /// The list for this version: the newest one at or below it, so TestFlight's 9.0.0 shows
    /// the newest list once, and a version without a list of its own shows nothing new.
    static func release(forAppVersion version: String, in releases: [WhatsNewRelease] = releases) -> WhatsNewRelease? {
        releases
            .filter { version.compare($0.version, options: .numeric) != .orderedAscending }
            .max { $0.version.compare($1.version, options: .numeric) == .orderedAscending }
    }

    static var current: WhatsNewRelease? {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
        return release(forAppVersion: version)
    }

    /// The list due at this launch, decided once per process as Home first shows. A first run
    /// (onboarding opened at launch, or resumed) marks the current list seen instead.
    static let dueAtLaunch: WhatsNewRelease? = {
        if snapshotShows { return current }
        guard !OnboardingFixture.isSnapshot else { return nil }
        return decideAtLaunch(current, defaults: .standard, firstRun: OnboardingProgress.launchPage != nil)
    }()

    static func decideAtLaunch(_ release: WhatsNewRelease?, defaults: UserDefaults, firstRun: Bool) -> WhatsNewRelease? {
        guard let release else { return nil }
        if firstRun {
            markSeen(release, defaults: defaults)
            return nil
        }
        return defaults.string(forKey: seenKey) == release.version ? nil : release
    }

    /// Only over Home itself, on an open no link asked for, with nothing else going on.
    static func shows(_ due: WhatsNewRelease?, at moment: WhatsNewMoment) -> WhatsNewRelease? {
        guard let due, moment.active, !moment.openedByLink, !moment.shownThisLaunch, !moment.setupSheet,
              !moment.settingsPage, !moment.presenting, !moment.dictationBusy else { return nil }
        return due
    }

    /// Seen as it shows, so a lock or a quit part-way never brings it back.
    static func markSeen(_ release: WhatsNewRelease, defaults: UserDefaults = .standard) {
        defaults.set(release.version, forKey: seenKey)
    }

    /// `--snap-state settings-whats-new`: Home with the sheet open.
    static var snapshotShows: Bool { SettingsSnapshotFixture.scene == "settings-whats-new" }

    /// This process's part of the moment: set by HEXMobileApp (links, back to the background)
    /// and by Home as the sheet shows.
    static var openedByLink = false
    static var shownThisLaunch = false

    static var somethingIsPresented: Bool {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .contains { $0.rootViewController?.presentedViewController != nil }
    }
}
