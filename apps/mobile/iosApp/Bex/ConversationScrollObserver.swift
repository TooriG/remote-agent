import SwiftUI
import UIKit

/// Own the native scroll view's status-bar navigation and history insertion offset.
struct ConversationScrollObserver: UIViewRepresentable {
    let controller: Coordinator
    let loadingHistory: Bool
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
        context.coordinator.update(loadingHistory: loadingHistory, followingLatest: followingLatest)
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

    final class Coordinator: NSObject, UIScrollViewDelegate {
        var onScrollToTop: () -> Void = {}
        weak var scrollView: UIScrollView?
        weak var original: UIScrollViewDelegate?

        private var contentSizeObservation: NSKeyValueObservation?
        private var historyAnchor: (offset: CGPoint, height: CGFloat)?
        private var loadingHistory = false
        private var followingLatest = true

        func captureReadingPosition() {
            guard let scrollView else { return }
            historyAnchor = (scrollView.contentOffset, scrollView.contentSize.height)
        }

        func cancelHistoryRestoration() {
            historyAnchor = nil
        }

        func update(loadingHistory: Bool, followingLatest: Bool) {
            self.loadingHistory = loadingHistory
            self.followingLatest = followingLatest
            if followingLatest {
                cancelHistoryRestoration()
            }
            restoreReadingPosition()
        }

        private func restoreReadingPosition() {
            guard !loadingHistory, historyAnchor != nil else { return }
            // Loading may finish before SwiftUI lays out the inserted rows.
            // Keep the anchor until UIKit publishes their new content height.
            DispatchQueue.main.async { [weak self] in
                guard let self, !self.loadingHistory, !self.followingLatest,
                      let scrollView else { return }
                scrollView.layoutIfNeeded()
                guard let anchor = historyAnchor, scrollView.contentSize.height != anchor.height else { return }
                historyAnchor = nil
                let offset = CGPoint(x: anchor.offset.x,
                                     y: anchor.offset.y + scrollView.contentSize.height - anchor.height)
                scrollView.setContentOffset(offset, animated: false)
            }
        }

        func attach(to view: UIView) {
            var ancestor = view.superview
            while let candidate = ancestor {
                if let scroll = candidate as? UIScrollView {
                    if scrollView !== scroll {
                        detach()
                        scrollView = scroll
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
            contentSizeObservation = nil
            cancelHistoryRestoration()
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
