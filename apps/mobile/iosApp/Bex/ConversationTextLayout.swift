import SwiftUI
import UIKit

/// One native text view keeps a single selection range across a whole message.
/// Like T3, it is as tall as the message and never scrolls itself: the conversation owns scrolling,
/// and UIKit tiles drawing to the visible part of a long message.
final class ConversationTextView: UITextView {
    static let quote = NSAttributedString.Key("agent.quote")
    static let rule = NSAttributedString.Key("agent.rule")

    private var content: ConversationTextContent?
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
        if let content, content.blocks == blocks, content.dark == dark, content.sizeCategory == sizeCategory {
            return
        }
        content = ConversationTextContent(blocks: blocks, dark: dark, sizeCategory: sizeCategory)
        idealWidth = nil
        displayedText = nil
        invalidateIntrinsicContentSize()
        setNeedsLayout()
    }

    func measure(width proposal: CGFloat?) -> CGSize {
        guard let content, !content.blocks.isEmpty else { return .zero }
        let width = proposal.flatMap { $0.isFinite ? $0 : nil }
        if width == nil, idealWidth == nil {
            idealWidth = ceil(ConversationAttributedText.attributedText(content.blocks, dark: content.dark,
                                                                        attachmentWidth: 300).size().width)
        }
        guard let width = width ?? idealWidth, width > 0 else { return .zero }
        return ConversationTextLayouts.layout(content, width: width).size
    }

    override func layoutSubviews() {
        if bounds.width > 0, let content {
            let text = ConversationTextLayouts.layout(content, width: bounds.width).text
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

/// The message a text view shows, compared by value. Its hash is computed once
/// because every size proposal looks it up.
private final class ConversationTextContent: NSObject {
    let blocks: [ConversationMarkdownContent.Block]
    let dark: Bool
    let sizeCategory: ContentSizeCategory
    private let digest: Int

    init(blocks: [ConversationMarkdownContent.Block], dark: Bool, sizeCategory: ContentSizeCategory) {
        self.blocks = blocks
        self.dark = dark
        self.sizeCategory = sizeCategory
        var hasher = Hasher()
        hasher.combine(blocks)
        hasher.combine(dark)
        hasher.combine(sizeCategory)
        digest = hasher.finalize()
    }

    override var hash: Int {
        digest
    }

    override func isEqual(_ object: Any?) -> Bool {
        guard let other = object as? ConversationTextContent else { return false }
        return other === self || other.digest == digest && other.dark == dark
            && other.sizeCategory == sizeCategory && other.blocks == blocks
    }
}

/// Text laid out for one width, shared by every view showing the same content.
/// Opening a conversation again neither rebuilds nor remeasures its messages, and
/// sizing reuses one measuring view instead of creating a text view per proposal.
@MainActor
private enum ConversationTextLayouts {
    final class Layout {
        let text: NSAttributedString
        let size: CGSize

        init(text: NSAttributedString, size: CGSize) {
            self.text = text
            self.size = size
        }
    }

    private final class Key: NSObject {
        let content: ConversationTextContent
        let width: CGFloat

        init(_ content: ConversationTextContent, width: CGFloat) {
            self.content = content
            self.width = width
        }

        override var hash: Int {
            content.hash ^ width.hashValue
        }

        override func isEqual(_ object: Any?) -> Bool {
            guard let other = object as? Key else { return false }
            return other.width == width && other.content.isEqual(content)
        }
    }

    /// Bounded and released under memory pressure, so a resized window does not
    /// keep a document copy for every proposed width.
    private static let cache: NSCache<Key, Layout> = {
        let cache = NSCache<Key, Layout>()
        cache.countLimit = 1000
        return cache
    }()

    /// Size proposals must not change a live view's text container or selection.
    private static let measuring: UITextView = {
        let view = UITextView(usingTextLayoutManager: false)
        view.textContainerInset = .zero
        view.textContainer.lineFragmentPadding = 0
        view.isScrollEnabled = false
        return view
    }()

    static func layout(_ content: ConversationTextContent, width: CGFloat) -> Layout {
        let key = Key(content, width: width)
        if let cached = cache.object(forKey: key) {
            return cached
        }
        let text = ConversationAttributedText.attributedText(content.blocks, dark: content.dark,
                                                             attachmentWidth: width)
        measuring.attributedText = text
        let measured = measuring.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        let layout = Layout(text: text, size: CGSize(width: width, height: measured.height))
        cache.setObject(layout, forKey: key)
        return layout
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
