import SwiftUI
import UIKit

/// Plain text stands in for Markdown, with ">" marking quoted lines; drawing and scrolling use the production view.
enum ConversationMarkdownContent {
    typealias Block = String
}

enum ConversationAttributedText {
    static func attributedText(_ blocks: [String], dark _: Bool, attachmentWidth _: CGFloat) -> NSAttributedString {
        let text = NSMutableAttributedString()
        for (index, block) in blocks.enumerated() {
            let quoted = block.hasPrefix(">")
            let paragraph = NSMutableParagraphStyle()
            paragraph.firstLineHeadIndent = quoted ? 12 : 0
            paragraph.headIndent = paragraph.firstLineHeadIndent
            var attributes: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: 18), .foregroundColor: UIColor.black, .paragraphStyle: paragraph
            ]
            if quoted {
                attributes[ConversationTextView.quote] = true
            }
            text.append(NSAttributedString(string: block + (index + 1 < blocks.count ? "\n" : ""),
                                           attributes: attributes))
        }
        return text
    }

    static func selectedText(_ text: NSAttributedString, range: NSRange) -> String {
        (text.string as NSString).substring(with: range)
    }
}

@main
struct ConversationTextViewTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    /// Count pixel rows that contain text and quote-bar pixels, rendered as the screen would show `rect`.
    @MainActor static func drawnRows(_ view: UIView, in rect: CGRect) -> (text: Int, quote: Int) {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = false
        format.preferredRange = .standard
        let image = UIGraphicsImageRenderer(bounds: rect, format: format).image { context in
            view.layer.render(in: context.cgContext)
        }
        guard let image = image.cgImage, let data = image.dataProvider?.data,
              let pixels = CFDataGetBytePtr(data) else { return (0, 0) }
        let rows = 0 ..< image.height
        // Quoted text starts at x 12; the bar sits left of it.
        return (rows.count { row in
            (12 ..< image.width).contains { pixels[row * image.bytesPerRow + $0 * 4 + 3] > 0 }
        }, rows.count { row in
            (0 ..< 4).contains { pixels[row * image.bytesPerRow + $0 * 4 + 3] > 0 }
        })
    }

    @MainActor static func main() {
        let blocks = (0 ..< 400).map { $0 >= 300 ? "> Quoted line \($0)" : "Line \($0) of a long message with words." }
        let view = ConversationTextView()
        view.update(blocks: blocks, dark: false, sizeCategory: .large)
        let size = view.measure(width: 320)
        check(size.height > 6000, "The message must extend far beyond one screen")
        view.frame = CGRect(origin: .zero, size: size)
        view.layoutIfNeeded()
        let quoted = view.layoutManager.boundingRect(
            forGlyphRange: view.layoutManager.glyphRange(
                forCharacterRange: (view.text as NSString).range(of: "> Quoted line 350"),
                actualCharacterRange: nil
            ),
            in: view.textContainer
        )
        let drawn = drawnRows(view, in: CGRect(x: 0, y: quoted.minY - 200, width: size.width, height: 400))
        check(drawn.text > 100, "Text far below the top of a long message must be drawn")
        check(drawn.quote > 100, "Quote bars far below the top of a long message must be drawn")

        check(!view.isScrollEnabled, "Drags on a message must scroll the conversation")
        let selection = NSRange(location: 4, length: 4)
        view.selectedRange = selection
        view.scrollRangeToVisible(NSRange(location: (view.text as NSString).length - 1, length: 1))
        check(view.contentOffset == .zero, "Revealing a selection must not scroll inside the message")
        _ = view.accessibilityScroll(.down)
        check(view.contentOffset == .zero, "Accessibility must not scroll inside the message")
        view.update(blocks: blocks + ["Streamed line"], dark: false, sizeCategory: .large)
        view.frame.size = view.measure(width: 320)
        view.layoutIfNeeded()
        check(view.text.hasSuffix("Streamed line"), "Streaming must show the new text")
        check(view.contentOffset == .zero, "Streaming text must not scroll inside the message")
        check(view.selectedRange == selection, "Streaming must keep the reader's selection")
        print("Conversation text view tests passed")
    }
}
