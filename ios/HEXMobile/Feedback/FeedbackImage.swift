import PhotosUI
import SwiftUI
import UIKit

/// The pictures a feedback can carry: Hex's own window, drawn just before the form opens (never
/// the whole screen), and photos the person picks, all re-encoded as JPEG at about 80% with the
/// longest side at most 2048 px.
enum FeedbackImage {
    /// Hex's key window as it is now, or nil.
    @MainActor
    static func keyWindow() -> UIImage? {
        let window = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first(where: \.isKeyWindow)
        guard let window, window.bounds.width > 0 else { return nil }
        let format = UIGraphicsImageRendererFormat(for: window.traitCollection)
        return UIGraphicsImageRenderer(bounds: window.bounds, format: format).image { _ in
            window.drawHierarchy(in: window.bounds, afterScreenUpdates: false)
        }
    }

    /// `image` shrunk and encoded, or nil when it can't be.
    nonisolated static func jpeg(_ image: UIImage) -> Data? {
        let pixels = CGSize(width: image.size.width * image.scale, height: image.size.height * image.scale)
        let longest = max(pixels.width, pixels.height)
        guard longest > 0 else { return nil }
        let factor = min(1, Feedback.imageSide / longest)
        let target = CGSize(width: (pixels.width * factor).rounded(), height: (pixels.height * factor).rounded())
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = true
        let drawn = UIGraphicsImageRenderer(size: target, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
        return drawn.jpegData(compressionQuality: Feedback.jpegQuality)
    }

    /// A picked photo (HEIC, PNG, anything UIImage reads) as a JPEG.
    nonisolated static func jpeg(fromPicked data: Data) -> Data? {
        UIImage(data: data).flatMap(jpeg)
    }
}
