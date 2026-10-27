import SwiftUI
import UIKit

/// One native text view keeps a single selection range across a whole message.
/// Like T3, it is as tall as the message and never scrolls itself: the conversation owns scrolling,
/// and UIKit tiles drawing to the visible part of a long message.
final class ConversationTextView: UITextView {
    static let quote = NSAttributedString.Key("agent.quote")
    static let rule = NSAttributedString.Key("agent.rule")

    private struct TextLayout {
        let text: NSAttributedString
        let size: CGSize
    }

    private var blocks: [ConversationMarkdownContent.Block] = []
    private var dark = false
    private var sizeCategory: ContentSizeCategory?
    private var layouts: [TextLayout] = []
    private var idealWidth: CGFloat?
    private var displayedText: NSAttributedString?
    /// UITextView does not retain storage that it did not create.
    private let storage = NSTextStorage()

    init() {
        // Decorations draw with the text, so they share its tiles instead of a message-sized backing store.
        let layoutManager = DecoratingLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: CGSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        container.lineFragmentPadding = 0
        layoutManager.addTextContainer(container)
        super.init(frame: .zero, textContainer: container)
        isScrollEnabled = false
        scrollsToTop = false
        isEditable = false
        isSelectable = true
        backgroundColor = .clear
        contentInsetAdjustmentBehavior = .never
        textContainerInset = .zero
        linkTextAttributes = [
            .foregroundColor: UIColor.systemBlue,
            .underlineStyle: NSUnderlineStyle.single.rawValue
        ]
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func update(blocks: [ConversationMarkdownContent.Block], dark: Bool, sizeCategory: ContentSizeCategory) {
        guard self.blocks != blocks || self.dark != dark || self.sizeCategory != sizeCategory else { return }
        self.blocks = blocks
        self.dark = dark
        self.sizeCategory = sizeCategory
        layouts.removeAll()
        idealWidth = nil
        displayedText = nil
        invalidateIntrinsicContentSize()
        setNeedsLayout()
    }

    func measure(width proposal: CGFloat?) -> CGSize {
        guard !blocks.isEmpty else { return .zero }
        let width = proposal.flatMap { $0.isFinite ? $0 : nil }
        if width == nil, idealWidth == nil {
            idealWidth = ceil(ConversationAttributedText.attributedText(blocks, dark: dark, attachmentWidth: 300).size()
                .width)
        }
        guard let width = width ?? idealWidth, width > 0 else { return .zero }
        return layout(width: width).size
    }

    private func layout(width: CGFloat) -> TextLayout {
        if let cached = layouts.first(where: { $0.size.width == width }) {
            return cached
        }
        let text = ConversationAttributedText.attributedText(blocks, dark: dark, attachmentWidth: width)
        // Size proposals must not change the live view's text container or selection.
        let measuring = UITextView(usingTextLayoutManager: false)
        measuring.textContainerInset = .zero
        measuring.textContainer.lineFragmentPadding = 0
        measuring.isScrollEnabled = false
        measuring.attributedText = text
        let measured = measuring.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        let result = TextLayout(text: text, size: CGSize(width: width, height: measured.height))
        // Resizing a window must not retain a document copy for every proposed width.
        if layouts.count == 3 {
            layouts.removeFirst()
        }
        layouts.append(result)
        return result
    }

    override func layoutSubviews() {
        if bounds.width > 0 {
            let text = layout(width: bounds.width).text
            if displayedText !== text {
                setContent(text)
                displayedText = text
            }
        }
        super.layoutSubviews()
    }

    private func setContent(_ content: NSAttributedString) {
        let range = selectedRange
        let keepSelection = range.length > 0 && NSMaxRange(range) <= textStorage.length
            && NSMaxRange(range) <= content.length
            && ConversationAttributedText.selectedText(attributedText, range: range) == ConversationAttributedText
            .selectedText(
                content,
                range: range
            )
        textStorage.setAttributedString(content)
        selectedRange = keepSelection ? range : NSRange(location: 0, length: 0)
    }
}

/// Draws quote bars and rules beneath the glyphs of each drawn tile.
private final class DecoratingLayoutManager: NSLayoutManager {
    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: CGPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let textStorage, let container = textContainers.first else { return }
        let characters = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        for attribute in [ConversationTextView.quote, ConversationTextView.rule] {
            textStorage.enumerateAttribute(attribute, in: characters) { value, range, _ in
                guard value as? Bool == true else { return }
                let glyphs = glyphRange(forCharacterRange: range, actualCharacterRange: nil)
                let bounds = boundingRect(forGlyphRange: glyphs, in: container)
                let line = UIBezierPath()
                if attribute == ConversationTextView.quote {
                    let lineX = max(1, bounds.minX - 10) + origin.x
                    line.move(to: CGPoint(x: lineX, y: bounds.minY + origin.y))
                    line.addLine(to: CGPoint(x: lineX, y: bounds.maxY + origin.y))
                    line.lineWidth = 2
                } else {
                    line.move(to: CGPoint(x: origin.x, y: bounds.midY + origin.y))
                    line.addLine(to: CGPoint(x: origin.x + container.size.width, y: bounds.midY + origin.y))
                    line.lineWidth = 0.5
                }
                UIColor.separator.setStroke()
                line.stroke()
            }
        }
    }
}
