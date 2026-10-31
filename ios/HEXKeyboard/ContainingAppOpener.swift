import UIKit

/// Opens a URL from the keyboard extension.
///
/// Keyboard extensions get no working URL opener: SwiftUI `Link`, the `openURL`
/// environment action, and `extensionContext.open` all silently do nothing in a
/// keyboard. The extension process still has a `UIApplication` at the end of its
/// responder chain, and its `open(_:options:completionHandler:)` works. That method
/// is marked unavailable to extensions at compile time, so it is called through the
/// Objective-C runtime. (`openURL:` alone is a no-op since iOS 18.)
@MainActor
enum ContainingAppOpener {
    private typealias OpenFunction = @convention(c) (
        AnyObject, Selector, NSURL, NSDictionary, AnyObject?
    ) -> Void

    @discardableResult
    static func open(_ url: URL, from responder: UIResponder) -> Bool {
        let selector = NSSelectorFromString("openURL:options:completionHandler:")
        guard let application = application(from: responder),
              application.responds(to: selector) else {
            return false
        }
        let implementation = application.method(for: selector)
        let open = unsafeBitCast(implementation, to: OpenFunction.self)
        open(application, selector, url as NSURL, NSDictionary(), nil)
        return true
    }

    private static func application(from responder: UIResponder) -> UIResponder? {
        var current: UIResponder? = responder
        while let candidate = current {
            if candidate.isKind(of: UIApplication.self) {
                return candidate
            }
            current = candidate.next
        }
        return (UIApplication.self as AnyObject).value(forKey: "sharedApplication") as? UIResponder
    }
}
