import Foundation
#if os(macOS)
    import AppKit
#else
    import UIKit
#endif

typealias MessageCallback = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void

/// TextKit owns selection, copying, hit testing and glyph measurement. NSH receives only its intrinsic size and actions.
@MainActor final class MessageTextOwner {
    let context: UnsafeMutableRawPointer?
    var callback: MessageCallback?
    let view = MessageTextContainer()

    init(context: UnsafeMutableRawPointer?, callback: @escaping MessageCallback) {
        self.context = context
        self.callback = callback
        let send: ([String: Any]) -> Void = { [weak self] action in
            guard let self, let data = try? JSONSerialization.data(withJSONObject: action) else { return }
            data.withUnsafeBytes { self.callback?(self.context, $0.bindMemory(to: UInt8.self).baseAddress, data.count) }
        }
        view.send = send
        view.text.action = send
    }

    func update(_ data: Data) {
        do { try view.update(JSONDecoder().decode(MessageDocument.self, from: data)) }
        catch {
            // Keep decoder failures visible rather than silently dropping a message.
            view.text.stringValueForFailure(error.localizedDescription)
        }
    }
}

#if os(macOS)
    final class MessageTextView: NSTextView, NSTextViewDelegate {
        var action: (([String: Any]) -> Void)?
        var document = MessageDocument.empty

        init() {
            let storage = NSTextStorage()
            let manager = NSLayoutManager()
            let container = NSTextContainer(size: .zero)
            storage.addLayoutManager(manager)
            manager.addTextContainer(container)
            super.init(frame: .zero, textContainer: container)
            isEditable = false
            isSelectable = true
            drawsBackground = false
            isRichText = true
            isHorizontallyResizable = false
            isVerticallyResizable = false
            textContainerInset = .zero
            textContainer?.lineFragmentPadding = 0
            textContainer?.widthTracksTextView = false
            delegate = self
            setAccessibilityIdentifier("message.assistant-text")
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) is unavailable")
        }

        override func menu(for event: NSEvent) -> NSMenu? {
            guard let menu = super.menu(for: event) else { return nil }
            let range = selectedRange()
            guard range.length > 0, NSMaxRange(range) <= (textStorage?.length ?? 0) else { return menu }
            let selected = (string as NSString).substring(with: range)
            let target = document.target
            let quote = NSMenuItem(title: "チャットに追加", action: #selector(quoteSelection(_:)), keyEquivalent: "")
            quote.target = self
            quote.representedObject = ["action": "quote", "text": selected, "target": target]
            menu.insertItem(quote, at: 0)
            if document.sideChat {
                let side = NSMenuItem(title: "サイドチャットで質問", action: #selector(quoteSelection(_:)), keyEquivalent: "")
                side.target = self
                side.representedObject = ["action": "sideChat", "text": selected, "target": target]
                menu.insertItem(side, at: 1)
            }
            menu.insertItem(.separator(), at: document.sideChat ? 2 : 1)
            return menu
        }

        @objc private func quoteSelection(_ sender: NSMenuItem) {
            guard let payload = sender.representedObject as? [String: Any] else { return }
            action?(payload)
        }

        func textView(_: NSTextView, clickedOnLink link: Any, at _: Int) -> Bool {
            let url = (link as? URL)?.absoluteString ?? (link as? String) ?? ""
            action?(["action": "link", "text": url, "target": document.target])
            return true
        }

        func stringValueForFailure(_ reason: String) {
            string = reason
        }
    }

    final class MessageTextContainer: NSView {
        let text = MessageTextView()
        var send: (([String: Any]) -> Void)?
        private var measuredSize = CGSize(width: -1, height: -1)
        override var isFlipped: Bool {
            true
        }

        init() {
            super.init(frame: .zero)
            addSubview(text)
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) is unavailable")
        }

        func update(_ document: MessageDocument) {
            guard text.document != document else { return }
            text.document = document
            refresh()
        }

        override func viewDidChangeEffectiveAppearance() {
            super.viewDidChangeEffectiveAppearance(); refresh()
        }

        private func reportSize(_ size: CGSize) {
            guard measuredSize != size else { return }
            measuredSize = size
            send?(["measuredWidth": size.width, "measuredHeight": size.height])
        }

        private func refresh() {
            let document = text.document
            let content = MessageAttributes.text(document.blocks, secondary: document.secondary,
                                                 dark: effectiveAppearance
                                                     .bestMatch(from: [.darkAqua, .aqua]) == .darkAqua)
            let range = text.selectedRange()
            let keep = range.length > 0 && NSMaxRange(range) <= content.length && NSMaxRange(range) <= text.string.utf16
                .count
                && (text.string as NSString).substring(with: range) == (content.string as NSString)
                .substring(with: range)
            text.textStorage?.setAttributedString(content)
            let size = content.size()
            reportSize(CGSize(width: ceil(size.width), height: ceil(size.height)))
            text.setSelectedRange(keep ? range : NSRange(location: 0, length: 0))
            needsLayout = true
        }

        override func layout() {
            super.layout()
            guard bounds.width > 0, let container = text.textContainer, let manager = text.layoutManager else { return }
            container.containerSize = CGSize(width: bounds.width, height: .greatestFiniteMagnitude)
            manager.ensureLayout(for: container)
            let height = ceil(manager.usedRect(for: container).maxY)
            text.frame = CGRect(x: 0, y: 0, width: bounds.width, height: height)
            reportSize(CGSize(width: max(0, measuredSize.width), height: height))
        }
    }
#else
    final class MessageTextView: UITextView, UITextViewDelegate {
        var action: (([String: Any]) -> Void)?
        var document = MessageDocument.empty

        init() {
            super.init(frame: .zero, textContainer: nil)
            isEditable = false
            isSelectable = true
            isScrollEnabled = false
            backgroundColor = .clear
            textContainerInset = .zero
            textContainer.lineFragmentPadding = 0
            delegate = self
            accessibilityIdentifier = "message.assistant-text"
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) is unavailable")
        }

        func textView(_ textView: UITextView, editMenuForTextIn range: NSRange,
                      suggestedActions: [UIMenuElement]) -> UIMenu? {
            guard range.length > 0, NSMaxRange(range) <= textView.attributedText.length else { return nil }
            let selected = (textView.text as NSString).substring(with: range)
            let target = document.target
            var actions: [UIMenuElement] = [UIAction(title: "チャットに追加", image: UIImage(systemName: "bubble")) {
                [weak self] _ in self?.action?(["action": "quote", "text": selected, "target": target])
            }]
            if document.sideChat {
                actions.append(UIAction(
                    title: "サイドチャットで質問",
                    image: UIImage(systemName: "bubble.left.and.bubble.right")
                ) {
                    [weak self] _ in self?.action?(["action": "sideChat", "text": selected, "target": target])
                })
            }
            return UIMenu(children: actions + suggestedActions)
        }

        func textView(_: UITextView, primaryActionFor textItem: UITextItem, defaultAction: UIAction) -> UIAction? {
            guard case let .link(url) = textItem.content else { return defaultAction }
            let target = document.target
            return UIAction { [weak self] _ in
                self?.action?(["action": "link", "text": url.absoluteString, "target": target])
            }
        }

        func stringValueForFailure(_ reason: String) {
            text = reason
        }
    }

    final class MessageTextContainer: UIView {
        let text = MessageTextView()
        var send: (([String: Any]) -> Void)?
        private var measuredSize = CGSize(width: -1, height: -1)

        init() {
            super.init(frame: .zero)
            addSubview(text)
            registerForTraitChanges([UITraitUserInterfaceStyle.self, UITraitPreferredContentSizeCategory.self]) {
                (view: MessageTextContainer, _: UITraitCollection) in view.refresh()
            }
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) is unavailable")
        }

        func update(_ document: MessageDocument) {
            guard text.document != document else { return }
            text.document = document
            refresh()
        }

        private func reportSize(_ size: CGSize) {
            guard measuredSize != size else { return }
            measuredSize = size
            send?(["measuredWidth": size.width, "measuredHeight": size.height])
        }

        private func refresh() {
            let document = text.document
            let content = MessageAttributes.text(document.blocks, secondary: document.secondary,
                                                 dark: traitCollection.userInterfaceStyle == .dark)
            let range = text.selectedRange
            let keep = range.length > 0 && NSMaxRange(range) <= content.length && NSMaxRange(range) <= text
                .attributedText.length
                && (text.text as NSString).substring(with: range) == (content.string as NSString).substring(with: range)
            text.attributedText = content
            let size = content.size()
            reportSize(CGSize(width: ceil(size.width), height: ceil(size.height)))
            text.selectedRange = keep ? range : NSRange(location: 0, length: 0)
            setNeedsLayout()
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            guard bounds.width > 0 else { return }
            let height = ceil(text.sizeThatFits(CGSize(width: bounds.width, height: .greatestFiniteMagnitude)).height)
            text.frame = CGRect(x: 0, y: 0, width: bounds.width, height: height)
            reportSize(CGSize(width: max(0, measuredSize.width), height: height))
        }
    }
#endif

@_cdecl("bex_nsh_message_text_create")
@MainActor public func messageTextCreate(
    _ context: UnsafeMutableRawPointer?,
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void
) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(MessageTextOwner(context: context, callback: callback)).toOpaque()
}

@_cdecl("bex_nsh_message_text_view")
@MainActor public func messageTextView(_ handle: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    Unmanaged.passUnretained(Unmanaged<MessageTextOwner>.fromOpaque(handle).takeUnretainedValue().view).toOpaque()
}

@_cdecl("bex_nsh_message_text_update")
@MainActor public func messageTextUpdate(
    _ handle: UnsafeMutableRawPointer,
    _ bytes: UnsafePointer<UInt8>,
    _ count: Int
) {
    Unmanaged<MessageTextOwner>.fromOpaque(handle).takeUnretainedValue().update(Data(bytes: bytes, count: count))
}

@_cdecl("bex_nsh_message_text_release")
@MainActor public func messageTextRelease(_ handle: UnsafeMutableRawPointer) {
    let owner = Unmanaged<MessageTextOwner>.fromOpaque(handle).takeRetainedValue()
    owner.callback = nil
    owner.view.send = nil
    owner.view.text.action = nil
}
