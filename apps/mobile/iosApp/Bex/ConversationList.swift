import Observation
import SwiftUI
import UIKit

@MainActor @Observable final class ConversationScrollState {
    struct Request: Equatable {
        let id = UUID()
        let edge: VerticalEdge
    }

    var following = true
    var latestVisible = true
    private(set) var request: Request?

    func goTo(_ edge: VerticalEdge) {
        following = edge == .bottom
        request = Request(edge: edge)
    }
}

struct ConversationList: UIViewControllerRepresentable {
    enum RowID: Hashable {
        case history
        case message(String)
    }

    let ids: [RowID]
    let scroll: ConversationScrollState
    let following: Bool
    let request: ConversationScrollState.Request?
    let accessibilityValue: String
    let row: (Int) -> AnyView

    func makeUIViewController(context _: Context) -> ConversationListController {
        ConversationListController(scroll: scroll)
    }

    func updateUIViewController(_ controller: ConversationListController, context _: Context) {
        controller.update(ids: ids, following: following, request: request,
                          accessibilityValue: accessibilityValue, row: row)
    }

    static func dismantleUIViewController(_ controller: ConversationListController, coordinator _: ()) {
        controller.stop()
    }
}

final class ConversationListController: UICollectionViewController {
    private struct Source {
        let ids: [ConversationList.RowID]
        let row: (Int) -> AnyView
    }

    private let scroll: ConversationScrollState
    private let layout = ConversationListLayout()
    private var source = Source(ids: [], row: { _ in AnyView(EmptyView()) })
    private var indices: [ConversationList.RowID: Int] = [:]
    private var pending: Source?
    private var applying = false
    private var active = true
    private var requestedEdge: VerticalEdge? = .bottom
    private var request: ConversationScrollState.Request?
    private var visibilityUpdate: Task<Void, Never>?
    private var data: UICollectionViewDiffableDataSource<Int, ConversationList.RowID>!

    init(scroll: ConversationScrollState) {
        self.scroll = scroll
        super.init(collectionViewLayout: layout)
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        collectionView.backgroundColor = .clear
        collectionView.alwaysBounceVertical = true
        collectionView.keyboardDismissMode = .interactive
        collectionView.isPrefetchingEnabled = false
        collectionView.selfSizingInvalidation = .enabledIncludingConstraints
        collectionView.accessibilityIdentifier = "task.detail"
        collectionView.register(ConversationListCell.self, forCellWithReuseIdentifier: "row")
        data = UICollectionViewDiffableDataSource(collectionView: collectionView) { [weak self] view, path, id in
            guard let cell = view.dequeueReusableCell(withReuseIdentifier: "row", for: path)
                as? ConversationListCell else { preconditionFailure("Unexpected conversation cell type") }
            self?.show(cell, id: id)
            return cell
        }
        layout.indexPath = { [weak self] id in self?.data.indexPath(for: id) }
    }

    func update(ids: [ConversationList.RowID], following: Bool, request: ConversationScrollState.Request?,
                accessibilityValue: String, row: @escaping (Int) -> AnyView) {
        loadViewIfNeeded()
        layout.following = following
        if following {
            layout.anchor = nil
        }
        if self.request != request {
            self.request = request
            requestedEdge = request?.edge
            layout.anchor = nil
        }
        collectionView.accessibilityValue = accessibilityValue
        pending = Source(ids: ids, row: row)
        applyPending()
    }

    private func captureAnchor(allowed: [ConversationList.RowID: Int]) {
        layout.anchor = collectionView.indexPathsForVisibleItems.sorted()
            .compactMap { path -> ConversationListLayout.Anchor? in
                guard let id = data.itemIdentifier(for: path), case .message = id,
                      allowed[id] != nil,
                      let attributes = layout.layoutAttributesForItem(at: path) else { return nil }
                return ConversationListLayout.Anchor(id: id,
                                                     displacement: attributes.frame.minY - collectionView.contentOffset
                                                         .y)
            }.first
    }

    private func applyPending() {
        guard !applying, let pending, collectionView.bounds.width > 32, collectionView.bounds.height > 0 else { return }
        self.pending = nil
        let changed = source.ids != pending.ids
        let nextIndices = changed
            ? Dictionary(uniqueKeysWithValues: pending.ids.enumerated().map { ($0.element, $0.offset) }) : indices
        if !layout.following, requestedEdge == nil,
           layout.anchor.flatMap({ nextIndices[$0.id] }) == nil {
            captureAnchor(allowed: nextIndices)
        }
        source = pending
        if changed {
            indices = nextIndices
            if layout.following {
                requestedEdge = .bottom
            }
            applying = true
            var snapshot = NSDiffableDataSourceSnapshot<Int, ConversationList.RowID>()
            snapshot.appendSections([0])
            snapshot.appendItems(source.ids)
            data.apply(snapshot, animatingDifferences: false) { [weak self] in
                guard let self, active else { return }
                applying = false
                refreshVisibleRows()
                restoreReadingPosition()
                applyPending()
            }
        } else {
            refreshVisibleRows()
        }
    }

    private func refreshVisibleRows() {
        guard active else { return }
        for path in collectionView.indexPathsForVisibleItems {
            if let cell = collectionView.cellForItem(at: path) as? ConversationListCell,
               let id = data.itemIdentifier(for: path) {
                show(cell, id: id)
            }
        }
        scrollToRequestedEdge()
        publishVisibility()
    }

    private func show(_ cell: ConversationListCell, id: ConversationList.RowID) {
        guard let index = indices[id] else { cell.unmount(); return }
        cell.show(AnyView(source.row(index).id(id)), parent: self)
    }

    override func collectionView(_: UICollectionView, willDisplay cell: UICollectionViewCell,
                                 forItemAt indexPath: IndexPath) {
        if let cell = cell as? ConversationListCell, !cell.isMounted, let id = data.itemIdentifier(for: indexPath) {
            show(cell, id: id)
        }
    }

    override func collectionView(_: UICollectionView, didEndDisplaying cell: UICollectionViewCell,
                                 forItemAt _: IndexPath) {
        (cell as? ConversationListCell)?.unmount()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        applyPending()
        scrollToRequestedEdge()
        if !layout.following, layout.anchor == nil {
            captureAnchor(allowed: indices)
        }
        publishVisibility()
    }

    private func scrollToRequestedEdge() {
        guard !applying, !source.ids.isEmpty, let edge = requestedEdge else { return }
        requestedEdge = nil
        collectionView.layoutIfNeeded()
        let offset = edge == .bottom ? layout.bottomOffset(in: collectionView) : -collectionView.adjustedContentInset
            .top
        collectionView.setContentOffset(
            CGPoint(x: -collectionView.adjustedContentInset.left, y: offset),
            animated: false
        )
    }

    private func restoreReadingPosition() {
        guard !layout.following, layout.anchor != nil else { return }
        collectionView.layoutIfNeeded()
        collectionView.setContentOffset(
            layout.targetContentOffset(forProposedContentOffset: collectionView.contentOffset),
            animated: false
        )
    }

    private func latestIsVisible() -> Bool {
        guard !source.ids.isEmpty,
              let last = layout.layoutAttributesForItem(at: IndexPath(item: source.ids.count - 1, section: 0))
        else { return true }
        let insets = collectionView.adjustedContentInset
        let viewport = collectionView.bounds.inset(by: insets)
        let marker = CGRect(x: last.frame.minX, y: last.frame.maxY - 80, width: last.frame.width, height: 80)
        return viewport.intersection(marker).height >= 0.8
    }

    private func publishVisibility() {
        guard active, visibilityUpdate == nil else { return }
        // Layout can run during a SwiftUI update; coalesce the observation write outside that update.
        visibilityUpdate = Task { @MainActor [weak self] in
            await Task.yield()
            guard !Task.isCancelled, let self else { return }
            visibilityUpdate = nil
            let visible = latestIsVisible()
            if scroll.latestVisible != visible {
                scroll.latestVisible = visible
            }
        }
    }

    func stop() {
        active = false
        pending = nil
        source = Source(ids: [], row: { _ in AnyView(EmptyView()) })
        indices.removeAll()
        visibilityUpdate?.cancel()
        visibilityUpdate = nil
        for cell in collectionView.visibleCells {
            (cell as? ConversationListCell)?.unmount()
        }
    }

    override func scrollViewDidScroll(_ scrollView: UIScrollView) {
        if !applying, !layout.following,
           scrollView.isTracking || scrollView.isDragging || scrollView.isDecelerating {
            captureAnchor(allowed: indices)
        }
        publishVisibility()
    }

    override func scrollViewDidChangeAdjustedContentInset(_: UIScrollView) {
        if layout.following {
            requestedEdge = .bottom
            view.setNeedsLayout()
        }
    }

    override func scrollViewDidEndDecelerating(_: UIScrollView) {
        if !layout.following {
            captureAnchor(allowed: indices)
        }
    }

    override func scrollViewDidEndScrollingAnimation(_: UIScrollView) {
        if !layout.following {
            captureAnchor(allowed: indices)
        }
    }

    override func scrollViewDidScrollToTop(_: UIScrollView) {
        if !layout.following {
            captureAnchor(allowed: indices)
        }
    }

    override func scrollViewWillBeginDragging(_: UIScrollView) {
        requestedEdge = nil
        scroll.following = false
        layout.following = false
        layout.anchor = nil
    }

    override func scrollViewWillEndDragging(_ scrollView: UIScrollView, withVelocity _: CGPoint,
                                            targetContentOffset: UnsafeMutablePointer<CGPoint>) {
        if scrollView.panGestureRecognizer.translation(in: scrollView).y < 0, latestIsVisible() {
            scroll.following = true
            layout.following = true
            targetContentOffset.pointee.y = layout.bottomOffset(in: scrollView)
        }
    }

    override func scrollViewShouldScrollToTop(_: UIScrollView) -> Bool {
        requestedEdge = nil
        scroll.following = false
        layout.following = false
        layout.anchor = nil
        return true
    }
}
