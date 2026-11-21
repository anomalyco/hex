import Foundation

/// What a keyboard process leaves until its keys are on screen. Until a keyboard's first
/// frame iOS shows only an empty panel, and it goes back to the previous keyboard when one is
/// slow to draw or ends while it starts (TestFlight build 56: "a strange rectangle", then the
/// original keyboard). So the text checker and supplementary lexicon wait for that frame,
/// and the optional work after them (Apple Speech's and Translation's language lists, the
/// capture page in-keyboard dictation preloads) also waits for memory to allow it.
nonisolated enum KeyboardLaunchWork {
    /// After the keyboard appears: its first frame is on screen by then.
    static let afterFirstFrame: TimeInterval = 0.3
    /// The optional work, a turn of its own after the text checker's.
    static let optionalWorkDelay: TimeInterval = 0.6
    /// Memory left below iOS's limit for the keyboard that the optional work needs: what the
    /// first in-keyboard dictation needs (`KeyboardLocalDictation.startHeadroomNeeded`), as
    /// the capture page is that dictation's start made early.
    static let optionalHeadroomNeeded = 32 * 1_048_576

    /// Never after a memory warning in this process; otherwise with enough headroom where
    /// iOS says (`os_proc_available_memory`, 0 where it doesn't, as in the simulator).
    static func allowsOptionalWork(headroom: Int, memoryWarned: Bool) -> Bool {
        guard !memoryWarned else { return false }
        return headroom == 0 || headroom >= optionalHeadroomNeeded
    }
}
