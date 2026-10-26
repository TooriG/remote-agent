import SwiftUI
import UIKit

/// Own the native scroll view's status-bar navigation and history insertion offset.
struct ConversationScrollObserver: UIViewRepresentable {
    let controller: Coordinator
    let followingLatest: Bool
    let onScrollToTop: () -> Void

    func makeCoordinator() -> Coordinator {
        controller
    }

    func makeUIView(context: Context) -> ObserverView {
        let view = ObserverView()
        view.isUserInteractionEnabled = false
        view.attach = { [weak coordinator = context.coordinator] view in coordinator?.attach(to: view) }
        return view
    }

    func updateUIView(_ view: ObserverView, context: Context) {
        context.coordinator.onScrollToTop = onScrollToTop
        context.coordinator.attach(to: view)
        if followingLatest {
            context.coordinator.cancelHistoryRestoration()
        }
    }

    static func dismantleUIView(_: ObserverView, coordinator: Coordinator) {
        coordinator.onScrollToTop = {}
        coordinator.detach()
    }

    final class ObserverView: UIView {
        var attach: ((UIView) -> Void)?
        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window != nil {
                attach?(self)
            }
        }
    }

    private struct ReadingPosition {
        let id: UUID
        var offset: CGPoint
        let height: CGFloat
        let turnCount: UInt64
        let hasMoreHistory: Bool
        var ready = false
    }

    final class Coordinator: NSObject, UIScrollViewDelegate {
        var onScrollToTop: () -> Void = {}
        weak var scrollView: UIScrollView?
        weak var original: UIScrollViewDelegate?
        private var historyAnchor: ReadingPosition?
        private var contentSizeObservation: NSKeyValueObservation?

        func captureReadingPosition(turnCount: UInt64, hasMoreHistory: Bool) -> UUID? {
            guard let scrollView else { return nil }
            let id = UUID()
            historyAnchor = ReadingPosition(id: id, offset: scrollView.contentOffset,
                                            height: scrollView.contentSize.height,
                                            turnCount: turnCount, hasMoreHistory: hasMoreHistory)
            return id
        }

        func cancelHistoryRestoration() {
            historyAnchor = nil
        }

        func historyLoaded(_ id: UUID?, succeeded: Bool, turnCount: UInt64, hasMoreHistory: Bool) {
            guard let id, var anchor = historyAnchor, anchor.id == id else { return }
            guard succeeded, turnCount != anchor.turnCount || hasMoreHistory != anchor.hasMoreHistory else {
                cancelHistoryRestoration()
                return
            }
            anchor.ready = true
            historyAnchor = anchor
            restoreReadingPosition()
        }

        private func restoreReadingPosition() {
            guard let anchor = historyAnchor, anchor.ready, let scrollView,
                  scrollView.contentSize.height != anchor.height else { return }
            historyAnchor = nil
            let offset = CGPoint(x: anchor.offset.x,
                                 y: anchor.offset.y + scrollView.contentSize.height - anchor.height)
            scrollView.setContentOffset(offset, animated: false)
        }

        func attach(to view: UIView) {
            var ancestor = view.superview
            while let candidate = ancestor {
                if let scroll = candidate as? UIScrollView {
                    if scrollView !== scroll {
                        detach()
                        scrollView = scroll
                        // The operation can finish before SwiftUI lays out its new rows.
                        contentSizeObservation = scroll.observe(\.contentSize) { [weak self] _, _ in
                            self?.restoreReadingPosition()
                        }
                    }
                    if scroll.delegate !== self {
                        original = scroll.delegate
                        scroll.delegate = self
                    }
                    return
                }
                ancestor = candidate.superview
            }
        }

        func detach() {
            cancelHistoryRestoration()
            contentSizeObservation = nil
            if scrollView?.delegate === self {
                scrollView?.delegate = original
            }
            scrollView = nil
            original = nil
        }

        override func responds(to selector: Selector!) -> Bool {
            super.responds(to: selector) || original?.responds(to: selector) == true
        }

        override func forwardingTarget(for _: Selector!) -> Any? {
            original
        }

        func scrollViewDidScroll(_ scrollView: UIScrollView) {
            if var anchor = historyAnchor, scrollView.contentSize.height == anchor.height {
                anchor.offset = scrollView.contentOffset
                historyAnchor = anchor
            }
            original?.scrollViewDidScroll?(scrollView)
        }

        func scrollViewShouldScrollToTop(_ scrollView: UIScrollView) -> Bool {
            let shouldScroll = original?.scrollViewShouldScrollToTop?(scrollView) ?? true
            if shouldScroll {
                cancelHistoryRestoration()
                onScrollToTop()
            }
            return shouldScroll
        }
    }
}
