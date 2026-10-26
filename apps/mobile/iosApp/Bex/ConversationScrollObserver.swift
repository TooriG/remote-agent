import UIKit

/// Observe native navigation and calculate the reading position after a history insertion.
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
    private var renderedPage: (page: HistoryPage, height: CGFloat)?

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

    func historyLoaded(_ id: UUID?, succeeded: Bool, turnCount: UInt64, hasMoreHistory: Bool) -> CGPoint? {
        guard let id, var anchor = historyAnchor, anchor.id == id else { return nil }
        let page = HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory)
        guard succeeded, page != anchor.page else {
            cancelHistoryRestoration()
            return nil
        }
        anchor.loadedPage = page
        historyAnchor = anchor
        return restoredReadingPosition()
    }

    func historyDidLayout(turnCount: UInt64, hasMoreHistory: Bool, contentHeight: CGFloat) -> CGPoint? {
        renderedPage = (HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory), contentHeight)
        return restoredReadingPosition()
    }

    private func restoredReadingPosition() -> CGPoint? {
        // Snapshot controls can update before the detached row projection.
        // Only the geometry of the completed page belongs to this insertion.
        guard let anchor = historyAnchor, let page = anchor.loadedPage,
              let renderedPage, page == renderedPage.page else { return nil }
        historyAnchor = nil
        // SwiftUI publishes geometry before the native content size necessarily catches up.
        // The screen applies this point through its existing ScrollPosition binding.
        return CGPoint(x: anchor.offset.x, y: anchor.offset.y + renderedPage.height - anchor.height)
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
