import UIKit

/// Own the native scroll view's status-bar navigation and history insertion offset.
final class ConversationScrollObserver: NSObject, UIScrollViewDelegate {
    private struct HistoryPage: Equatable {
        let turnCount: UInt64
        let hasMore: Bool
    }

    private struct ReadingPosition {
        let id: UUID
        var offset: CGPoint
        let height: CGFloat
        let page: HistoryPage
        var loadedPage: HistoryPage?
    }

    var onScrollToTop: () -> Void = {}
    private weak var scrollView: UIScrollView?
    private weak var original: UIScrollViewDelegate?
    private var historyAnchor: ReadingPosition?
    private var renderedPage: HistoryPage?

    func captureReadingPosition(turnCount: UInt64, hasMoreHistory: Bool) -> UUID? {
        guard let scrollView else { return nil }
        let id = UUID()
        historyAnchor = ReadingPosition(id: id, offset: scrollView.contentOffset,
                                        height: scrollView.contentSize.height,
                                        page: HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory))
        return id
    }

    func cancelHistoryRestoration() {
        historyAnchor = nil
    }

    func historyLoaded(_ id: UUID?, succeeded: Bool, turnCount: UInt64, hasMoreHistory: Bool) {
        guard let id, var anchor = historyAnchor, anchor.id == id else { return }
        let page = HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory)
        guard succeeded, page != anchor.page else {
            cancelHistoryRestoration()
            return
        }
        anchor.loadedPage = page
        historyAnchor = anchor
        restoreReadingPosition()
    }

    func historyDidLayout(turnCount: UInt64, hasMoreHistory: Bool) {
        renderedPage = HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory)
        restoreReadingPosition()
    }

    private func restoreReadingPosition() {
        // Snapshot controls can update before the detached row projection.
        // Only the geometry of the completed page belongs to this insertion.
        guard let anchor = historyAnchor, let page = anchor.loadedPage,
              page == renderedPage, let scrollView else { return }
        historyAnchor = nil
        let offset = CGPoint(x: anchor.offset.x,
                             y: anchor.offset.y + scrollView.contentSize.height - anchor.height)
        scrollView.setContentOffset(offset, animated: false)
    }

    func attach(to scroll: UIScrollView) {
        if scrollView !== scroll {
            detach()
            scrollView = scroll
        }
        if scroll.delegate !== self {
            original = scroll.delegate
            scroll.delegate = self
        }
    }

    func detach() {
        cancelHistoryRestoration()
        renderedPage = nil
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
