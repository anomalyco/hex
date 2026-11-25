import SwiftUI

/// What's new: a title, the items one under another, each a picture from the real app over
/// its name and sentence, and Continue kept at the bottom. Swiping the sheet down closes it too.
struct WhatsNewSheet: View {
    let release: WhatsNewRelease
    @Environment(\.dismiss) private var dismiss
    @State private var visibleHeight: CGFloat = 700

    /// A picture leaves room for its words: half the visible height, so in landscape a whole
    /// item fits, and at most 360 pt.
    private var pictureHeight: CGFloat {
        min(360, max(150, visibleHeight * 0.5))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text("What's new in Hex")
                    .font(.system(size: 28, weight: .semibold))
                    .tracking(-0.4)
                    .foregroundStyle(HexTheme.textBase)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.top, 36)
                    .padding(.bottom, 28)

                VStack(alignment: .leading, spacing: 36) {
                    ForEach(release.items) { item in
                        WhatsNewItemView(item: item, pictureHeight: pictureHeight)
                    }
                }
            }
            .frame(maxWidth: 500, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { visibleHeight = $0 }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            OnboardingPrimaryButton(title: "Continue") { dismiss() }
                .frame(maxWidth: 500)
                .padding(.horizontal, 20)
                .padding(.top, 12)
                .padding(.bottom, 8)
                .frame(maxWidth: .infinity)
                .background(HexTheme.bgDeep)
        }
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .presentationBackground(HexTheme.bgDeep)
        .preferredColorScheme(.dark)
    }
}

private struct WhatsNewItemView: View {
    let item: WhatsNewItem
    let pictureHeight: CGFloat

    private static let pictureShape = RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)

    /// VoiceOver reads the name, the sentence, then what the picture shows.
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Image(item.imageName)
                .resizable()
                .aspectRatio(contentMode: .fit)
                .clipShape(Self.pictureShape)
                .overlay(Self.pictureShape.strokeBorder(HexTheme.borderWeak, lineWidth: 0.5))
                .frame(maxWidth: .infinity, maxHeight: pictureHeight, alignment: .leading)
                .accessibilityLabel(Text(item.pictureLabel))
                .accessibilityAddTraits(.isImage)
                .accessibilitySortPriority(1)

            VStack(alignment: .leading, spacing: 4) {
                Text(item.title)
                    .font(.system(size: 17, weight: .semibold))
                    .tracking(-0.2)
                    .foregroundStyle(HexTheme.textBase)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilitySortPriority(3)
                Text(item.detail)
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilitySortPriority(2)
            }
            .padding(.horizontal, 4)
            .accessibilityElement(children: .contain)
            .accessibilitySortPriority(2)
        }
        .accessibilityElement(children: .contain)
    }
}
