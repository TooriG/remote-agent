import SwiftUI
import UIKit

/// Preserve one native selection range without allocating a document-sized backing store.
/// SwiftUI owns the full document height; UIKit draws only the visible part of it.
final class ConversationTextContainer: UIView {
    let textView = ConversationTextView(usingTextLayoutManager: false)
    private var blocks: [ConversationMarkdownContent.Block] = []
    private var dark = false
    private var sizeCategory: ContentSizeCategory?
    private struct TextLayout {
        let text: NSAttributedString
        let size: CGSize
    }

    private var layouts: [TextLayout] = []
    private var idealWidth: CGFloat?
    private var displayedText: NSAttributedString?
    private var observedScrollViews: [ObjectIdentifier] = []
    private var observations: [NSKeyValueObservation] = []

    override init(frame: CGRect) {
        super.init(frame: frame)
        textView.scrollsToTop = false
        textView.isEditable = false
        textView.isSelectable = true
        textView.panGestureRecognizer.isEnabled = false
        textView.showsVerticalScrollIndicator = false
        textView.showsHorizontalScrollIndicator = false
        textView.bounces = false
        textView.backgroundColor = .clear
        textView.contentInsetAdjustmentBehavior = .never
        textView.textContainerInset = .zero
        textView.textContainer.lineFragmentPadding = 0
        textView.textContainer.widthTracksTextView = false
        textView.linkTextAttributes = [
            .foregroundColor: UIColor.systemBlue,
            .underlineStyle: NSUnderlineStyle.single.rawValue
        ]
        clipsToBounds = true
        addSubview(textView)
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

    override func didMoveToWindow() {
        super.didMoveToWindow()
        observeScrollViews()
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        observeScrollViews()
        updateViewport()
    }

    private func ancestorScrollViews() -> [UIScrollView] {
        var result: [UIScrollView] = []
        var ancestor = superview
        while let view = ancestor {
            if let scroll = view as? UIScrollView {
                result.append(scroll)
            }
            ancestor = view.superview
        }
        return result
    }

    private func observeScrollViews() {
        let views = window == nil ? [] : ancestorScrollViews()
        let identities = views.map(ObjectIdentifier.init)
        guard identities != observedScrollViews else { return }
        observations.removeAll()
        observedScrollViews = identities
        observations = views.flatMap { scroll in
            [scroll.observe(\.contentOffset, options: [.new]) { [weak self] _, _ in self?.updateViewport() },
             scroll.observe(\.bounds, options: [.new]) { [weak self] _, _ in self?.updateViewport() }]
        }
    }

    private func updateViewport() {
        guard bounds.width > 0, bounds.height > 0, window != nil else { return }
        var viewport = bounds
        for scroll in ancestorScrollViews() {
            // Include a small margin so ordinary scrolling does not expose an empty tile.
            let visible = convert(scroll.bounds, from: scroll)
                .insetBy(dx: -scroll.bounds.width / 4, dy: -scroll.bounds.height / 4)
            viewport = viewport.intersection(visible)
        }
        guard !viewport.isNull, !viewport.isEmpty else {
            textView.isHidden = true
            return
        }
        let content = layout(width: bounds.width)
        if displayedText !== content.text {
            textView.textContainer.size = CGSize(width: bounds.width, height: .greatestFiniteMagnitude)
            textView.setContent(content.text)
            displayedText = content.text
        }
        textView.isHidden = false
        if textView.frame != viewport {
            textView.frame = viewport
        }
        if textView.contentSize != content.size {
            textView.contentSize = content.size
        }
        if textView.contentOffset != viewport.origin {
            textView.contentOffset = viewport.origin
        }
    }
}

/// Decorations stay in the same native text view as selectable prose.
final class ConversationTextView: UITextView {
    func setContent(_ content: NSAttributedString) {
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

    static let quote = NSAttributedString.Key("bex.quote")
    static let rule = NSAttributedString.Key("bex.rule")

    override func draw(_ rect: CGRect) {
        super.draw(rect)
        let glyphs = layoutManager.glyphRange(forBoundingRect: bounds, in: textContainer)
        let range = layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        for attribute in [Self.quote, Self.rule] {
            textStorage.enumerateAttribute(attribute, in: range) { value, range, _ in
                guard value as? Bool == true else { return }
                let glyphs = layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
                let bounds = layoutManager.boundingRect(forGlyphRange: glyphs, in: textContainer)
                let line = UIBezierPath()
                if attribute == Self.quote {
                    let lineX = max(1, bounds.minX - 10) + textContainerInset.left
                    line.move(to: CGPoint(x: lineX, y: bounds.minY + textContainerInset.top))
                    line.addLine(to: CGPoint(x: lineX, y: bounds.maxY + textContainerInset.top))
                    line.lineWidth = 2
                } else {
                    line.move(to: CGPoint(x: 0, y: bounds.midY + textContainerInset.top))
                    line.addLine(to: CGPoint(x: self.bounds.width, y: bounds.midY + textContainerInset.top))
                    line.lineWidth = 0.5
                }
                UIColor.separator.setStroke()
                line.stroke()
            }
        }
    }
}
