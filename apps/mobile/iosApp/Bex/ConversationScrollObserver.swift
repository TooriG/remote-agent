import UIKit

/// Observe native navigation and calculate the reading position after a history insertion.
final class ConversationScrollObserver: NSObject, UIScrollViewDelegate {
    private struct HistoryPage: Equatable {
        let turnCount: UInt64
        let hasMore: Bool
    }

    struct Viewport {
        let offset: CGPoint
        let origin: CGPoint
        let height: CGFloat
    }

    enum Target: Equatable {
        case point(CGPoint)
        case row(String)
    }

    private struct ReadingPosition {
        let id: UUID
        var row: String
        var position: CGPoint
        let page: HistoryPage
        var loadedPage: HistoryPage?
        var correctedFrame: CGRect?
        var realizing = false
    }

    private struct RenderedPage {
        let page: HistoryPage
        let rows: [String: CGRect]
        let viewport: Viewport?
    }

    var onScrollToTop: () -> Void = {}
    private weak var scrollView: UIScrollView?
    private weak var original: UIScrollViewDelegate?
    private var historyAnchor: ReadingPosition?
    private var renderedPage: RenderedPage?

    var viewport: Viewport? {
        guard let scrollView else { return nil }
        let insets = scrollView.adjustedContentInset
        return Viewport(offset: scrollView.contentOffset, origin: CGPoint(x: insets.left, y: insets.top),
                        height: scrollView.bounds.height)
    }

    func captureReadingPosition() -> UUID? {
        guard let renderedPage, let row = visibleRow(renderedPage) else { return nil }
        let id = UUID()
        historyAnchor = ReadingPosition(id: id, row: row.key, position: row.value.origin, page: renderedPage.page)
        return id
    }

    private func visibleRow(_ page: RenderedPage) -> Dictionary<String, CGRect>.Element? {
        guard let viewport = page.viewport else { return nil }
        return page.rows.filter { $0.value.maxY > 0 && $0.value.minY < viewport.height }
            .min { $0.value.minY < $1.value.minY }
    }

    func cancelHistoryRestoration() {
        historyAnchor = nil
    }

    func historyLoaded(_ id: UUID?, succeeded: Bool, turnCount: UInt64, hasMoreHistory: Bool) -> Target? {
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

    func historyDidLayout(turnCount: UInt64, hasMoreHistory: Bool, rows: [String: CGRect],
                          viewport: Viewport?) -> Target? {
        let page = RenderedPage(page: HistoryPage(turnCount: turnCount, hasMore: hasMoreHistory),
                                rows: rows, viewport: viewport)
        renderedPage = page
        if var anchor = historyAnchor, page.page == anchor.page, anchor.loadedPage == nil,
           let row = page.rows[anchor.row].map({ (key: anchor.row, value: $0) }) ?? visibleRow(page) {
            anchor.row = row.key
            anchor.position = row.value.origin
            historyAnchor = anchor
        }
        return restoredReadingPosition()
    }

    private func restoredReadingPosition() -> Target? {
        // Row frames are measured in the viewport. Estimated total heights in a
        // lazy stack cannot tell how far a visible message actually moved.
        guard var anchor = historyAnchor, let loaded = anchor.loadedPage,
              let page = renderedPage, page.page == loaded, let viewport = page.viewport else { return nil }
        guard let frame = page.rows[anchor.row] else {
            guard !anchor.realizing else { return nil }
            anchor.realizing = true
            historyAnchor = anchor
            return .row(anchor.row)
        }
        guard anchor.correctedFrame != frame else { return nil }
        anchor.correctedFrame = frame
        historyAnchor = anchor
        let delta = CGPoint(x: frame.minX - anchor.position.x, y: frame.minY - anchor.position.y)
        guard abs(delta.x) > 0.1 || abs(delta.y) > 0.1 else { return nil }
        return .point(CGPoint(x: viewport.offset.x + delta.x + viewport.origin.x,
                              y: viewport.offset.y + delta.y + viewport.origin.y))
    }

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
        if historyAnchor?.loadedPage != nil {
            cancelHistoryRestoration()
        }
        original?.scrollViewWillBeginDragging?(scrollView)
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
        if let page = renderedPage, page.viewport == nil {
            _ = historyDidLayout(turnCount: page.page.turnCount, hasMoreHistory: page.page.hasMore,
                                 rows: page.rows, viewport: viewport)
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
