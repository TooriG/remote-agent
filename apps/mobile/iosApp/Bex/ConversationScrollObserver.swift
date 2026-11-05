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

    private struct RenderedPage {
        let page: HistoryPage
        let height: CGFloat
        let offset: CGPoint
        let scrollOrigin: CGPoint
    }

    var onScrollToTop: () -> Void = {}
    private weak var scrollView: UIScrollView?
    private weak var original: UIScrollViewDelegate?
    private var historyAnchor: ReadingPosition?
    private var renderedPage: RenderedPage?

    func captureReadingPosition() -> UUID? {
        guard let renderedPage else { return nil }
        let id = UUID()
        historyAnchor = ReadingPosition(id: id, offset: renderedPage.offset,
                                        height: renderedPage.height, page: renderedPage.page)
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

    func historyDidLayout(turnCount: UInt64, hasMoreHistory: Bool,
                          contentHeight: CGFloat, contentOffset: CGPoint, scrollOrigin: CGPoint) -> CGPoint? {
        let page = RenderedPage(page: HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory),
                                height: contentHeight, offset: contentOffset, scrollOrigin: scrollOrigin)
        renderedPage = page
        if var anchor = historyAnchor, page.page == anchor.page, contentHeight == anchor.height {
            anchor.offset = contentOffset
            historyAnchor = anchor
        }
        return restoredReadingPosition()
    }

    private func restoredReadingPosition() -> CGPoint? {
        // Loaded rows are measured eagerly. Restore once, after the completed page is laid out.
        guard let anchor = historyAnchor, let loaded = anchor.loadedPage,
              let page = renderedPage, page.page == loaded else { return nil }
        historyAnchor = nil
        // ScrollPosition points start after the completed layout's viewport insets.
        return CGPoint(x: anchor.offset.x + page.scrollOrigin.x,
                       y: anchor.offset.y + page.height - anchor.height + page.scrollOrigin.y)
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

    func scrollViewShouldScrollToTop(_ scrollView: UIScrollView) -> Bool {
        let shouldScroll = original?.scrollViewShouldScrollToTop?(scrollView) ?? true
        if shouldScroll {
            cancelHistoryRestoration()
            onScrollToTop()
        }
        return shouldScroll
    }
}
