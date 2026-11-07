import SwiftUI
import UIKit

/// Plain text stands in for Markdown, with ">" marking quoted lines; drawing and scrolling use the production view.
enum ConversationMarkdownContent {
    typealias Block = String
}

enum ConversationAttributedText {
    static func attributedText(_ blocks: [String], dark: Bool) -> NSAttributedString {
        let text = NSMutableAttributedString()
        for (index, block) in blocks.enumerated() {
            let quoted = block.hasPrefix(">")
            let paragraph = NSMutableParagraphStyle()
            paragraph.firstLineHeadIndent = quoted ? 12 : 0
            paragraph.headIndent = paragraph.firstLineHeadIndent
            var attributes: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: 18), .foregroundColor: dark ? UIColor.white : UIColor.black,
                .paragraphStyle: paragraph
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
    private final class Edits: NSObject, NSTextStorageDelegate {
        var range: NSRange?

        func textStorage(_: NSTextStorage, didProcessEditing _: NSTextStorage.EditActions,
                         range editedRange: NSRange, changeInLength _: Int) {
            range = editedRange
        }
    }

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

    @MainActor static func main() async throws {
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
        let edits = Edits()
        view.textStorage.delegate = edits
        let lastParagraph = (view.text as NSString).range(of: blocks.last!).location
        view.update(blocks: blocks + ["Streamed line"], dark: false, sizeCategory: .large)
        check((edits.range?.location ?? 0) >= lastParagraph,
              "Appending a line must retain TextKit's layout for completed paragraphs")
        view.frame.size = view.measure(width: 320)
        view.layoutIfNeeded()
        check(view.text.hasSuffix("Streamed line"), "Streaming must show the new text")
        check(view.contentOffset == .zero, "Streaming text must not scroll inside the message")
        check(view.selectedRange == selection, "Streaming must keep the reader's selection")

        checkReopening(blocks: blocks + ["Streamed line"], previousSize: view.frame.size)
        checkRestyling()
        #if AGENT_DEV_DIAGNOSTICS
            try await checkDiagnostics()
        #endif
        print("Conversation text view tests passed")
    }

    @MainActor private static func checkReopening(blocks: [String], previousSize: CGSize) {
        // Reopened conversations share measurements, keyed by what the view shows.
        let reopened = ConversationTextView()
        reopened.update(blocks: blocks, dark: false, sizeCategory: .large)
        check(reopened.measure(width: 320) == previousSize, "The same message must measure the same when reopened")
        check(reopened.measure(width: 160).height > previousSize.height, "A narrower width must be measured again")
        let short = Array(blocks.prefix(10))
        reopened.update(blocks: short, dark: false, sizeCategory: .large)
        reopened.frame.size = reopened.measure(width: 320)
        check(reopened.frame.height < previousSize.height / 10, "Changed text must not reuse an old size")
        reopened.layoutIfNeeded()
        check(reopened.text == short.joined(separator: "\n"), "Changed text must not show an old layout")
        reopened.update(blocks: ["Short answer"], dark: false, sizeCategory: .large)
        check(reopened.measure(width: 320).width == 320,
              "Short messages must fill the proposed column width instead of shrinking to their text")
    }

    @MainActor private static func checkRestyling() {
        let view = ConversationTextView()
        view.update(blocks: ["First paragraph", "tail"], dark: false, sizeCategory: .large)
        view.update(blocks: ["> First paragraph", "tail extended"], dark: false, sizeCategory: .large)
        check(view.textStorage.attribute(ConversationTextView.quote, at: 0, effectiveRange: nil) as? Bool == true,
              "Changed attributes before the streamed tail must be updated")
        view.update(blocks: ["> First paragraph", "tail extended"], dark: true, sizeCategory: .large)
        check(view.textStorage.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor == .white,
              "Attribute-only changes must restyle existing text")
        view.update(blocks: ["日本語 😀", "tail"], dark: false, sizeCategory: .large)
        view.update(blocks: ["日本語 😃", "tail extended"], dark: false, sizeCategory: .large)
        check(view.text == "日本語 😃\ntail extended", "Tail replacement must preserve Unicode boundaries")
    }

    #if AGENT_DEV_DIAGNOSTICS
        @MainActor private static func checkDiagnostics() async throws {
            let body = "本文をログに記録しない検査\n最終行まで配置される必要があります。"
            let view = ConversationTextView()
            view.update(blocks: [body], dark: false, sizeCategory: .large)
            let size = view.measure(width: 320)
            view.frame.size = size
            view.layoutIfNeeded()
            _ = drawnRows(view, in: view.bounds)
            let complete = view.textLayoutDiagnostic()
            check(!complete.incomplete, "Fully drawn text must not report missing characters")
            checkClippedWhitespace(body: body, size: size)

            let parent = UIView(frame: CGRect(x: 0, y: 0, width: size.width, height: size.height - 1))
            parent.clipsToBounds = true
            parent.addSubview(view)
            let clipped = view.textLayoutDiagnostic()
            check(clipped.placedEnd == clipped.requiredEnd && clipped.incomplete,
                  "A parent hiding the last line must be detected even when every character is laid out")
            view.removeFromSuperview()
            let scrolling = UIScrollView(frame: parent.frame)
            scrolling.addSubview(view)
            check(!view.textLayoutDiagnostic().incomplete,
                  "Scrolling intentionally hides text and must not report a clipped message")
            view.removeFromSuperview()

            view.frame.size.height -= 1
            view.layoutIfNeeded()
            _ = drawnRows(view, in: view.bounds)
            let container = view.textContainer.size
            let offset = view.contentOffset
            let incomplete = view.textLayoutDiagnostic()
            check(incomplete.incomplete, "A clipped last line must be detected from the existing layout")
            check(incomplete.referenceHeight.map { $0 > view.bounds.height } == true,
                  "An independent layout must show the height the text needs")
            check(view.textContainer.size == container && view.contentOffset == offset && view.text == body,
                  "Diagnostics must not fix or otherwise change the layout being investigated")
            try await checkDiagnosticLog(complete: complete, incomplete: incomplete)
        }

        @MainActor private static func checkClippedWhitespace(body: String, size: CGSize) {
            let view = ConversationTextView()
            view.update(blocks: [body + "\n\n"], dark: false, sizeCategory: .large)
            view.frame.size = size
            view.layoutIfNeeded()
            _ = drawnRows(view, in: view.bounds)
            check(!view.textLayoutDiagnostic().incomplete,
                  "Clipped trailing whitespace must not be treated as missing visible text")
        }

        private static func checkDiagnosticLog(
            complete: ConversationTextDiagnostic,
            incomplete: ConversationTextDiagnostic
        ) async throws {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: directory) }
            let log = ConversationTextDiagnosticLog(directory: directory, byteLimit: 4096)
            await log.record(complete)
            await log.record(complete)
            await log.record(incomplete)
            await log.record(complete, recovered: true)
            let current = directory.appendingPathComponent("text-layout.jsonl")
            let data = try Data(contentsOf: current)
            let text = String(data: data, encoding: .utf8)
            check(text?.split(separator: "\n").count == 3, "Log one normal baseline, an anomaly, and its recovery")
            check(text?.contains("本文をログに記録しない検査") == false,
                  "Layout diagnostics must never contain message text")
            await log.record(complete, snapshot: true)
            let snapshot = try Data(contentsOf: current)
            check(String(data: snapshot, encoding: .utf8)?.split(separator: "\n").count == 4,
                  "An explicit diagnostic snapshot can record normal geometry without enabling routine logging")
            for _ in 0 ..< 30 {
                await log.record(incomplete)
            }
            for name in ["text-layout.jsonl", "text-layout.previous.jsonl"] {
                let bytes = try Data(contentsOf: directory.appendingPathComponent(name))
                check(bytes.count <= 4096, "Rotated diagnostic files must stay bounded")
            }
            try await checkDiagnosticWriteRecovery(complete, directory: directory)
        }

        private static func checkDiagnosticWriteRecovery(
            _ metrics: ConversationTextDiagnostic,
            directory: URL
        ) async throws {
            let unavailable = directory.appendingPathComponent("temporarily-unavailable")
            try Data().write(to: unavailable)
            let log = ConversationTextDiagnosticLog(directory: unavailable)
            await log.record(metrics)
            try FileManager.default.removeItem(at: unavailable)
            await log.record(metrics)
            check(FileManager.default.fileExists(atPath: unavailable.appendingPathComponent("text-layout.jsonl").path),
                  "A failed diagnostic write must recover without breaking text display")
        }
    #endif
}
