#if os(macOS)
    import AppKit
    import SwiftUI

    public struct RemoteBrowserCanvas: NSViewRepresentable {
        let image: NSImage
        let acceptsInput: Bool
        let send: (BrowserPointerAction) -> Void

        public init(image: NSImage, acceptsInput: Bool, send: @escaping (BrowserPointerAction) -> Void) {
            self.image = image
            self.acceptsInput = acceptsInput
            self.send = send
        }

        public func makeNSView(context _: Context) -> BrowserCanvasView {
            BrowserCanvasView()
        }

        public func updateNSView(_ view: BrowserCanvasView, context _: Context) {
            view.display(image)
            view.send = acceptsInput ? send : nil
        }
    }

    public final class BrowserCanvasView: NSScrollView {
        private let page = NSImageView()
        var send: ((BrowserPointerAction) -> Void)?
        private var previousSize = CGSize.zero

        public init() {
            super.init(frame: .zero)
            documentView = page
            allowsMagnification = true
            hasVerticalScroller = true
            hasHorizontalScroller = true
            page.addGestureRecognizer(NSClickGestureRecognizer(target: self, action: #selector(tapped)))
            setAccessibilityLabel("ホストのBexブラウザ")
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        func display(_ image: NSImage) {
            let changed = page.image?.size != image.size
            page.image = image
            if changed {
                page.frame = CGRect(origin: .zero, size: image.size)
                previousSize = .zero
                needsLayout = true
            }
        }

        override public func layout() {
            super.layout()
            guard bounds.size != previousSize, bounds.width > 0, page.bounds.width > 0 else { return }
            previousSize = bounds.size
            minMagnification = min(1, bounds.width / page.bounds.width)
            maxMagnification = max(2, minMagnification * 4)
            magnification = minMagnification
        }

        override public func scrollWheel(with event: NSEvent) {
            guard let send else { super.scrollWheel(with: event); return }
            let point = page.convert(event.locationInWindow, from: nil)
            guard page.bounds.contains(point) else { return }
            let scale = event.hasPreciseScrollingDeltas ? 1.0 : 20.0
            send(.scroll(x: point.x, y: page.bounds.height - point.y,
                         deltaX: max(-4096, min(4096, -event.scrollingDeltaX * scale)),
                         deltaY: max(-4096, min(4096, -event.scrollingDeltaY * scale))))
        }

        @objc private func tapped(_ gesture: NSClickGestureRecognizer) {
            let point = gesture.location(in: page)
            guard page.bounds.contains(point) else { return }
            send?(.click(x: point.x, y: page.bounds.height - point.y))
        }
    }
#endif
