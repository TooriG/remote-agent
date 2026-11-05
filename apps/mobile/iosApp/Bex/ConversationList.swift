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
        data = UICollectionViewDiffableDataSource(collectionView: collectionView) { [weak self] view, path, _ in
            guard let cell = view.dequeueReusableCell(withReuseIdentifier: "row", for: path)
                as? ConversationListCell else { preconditionFailure("Unexpected conversation cell type") }
            self?.show(cell, at: path)
            return cell
        }
        layout.indexPath = { [weak self] id in self?.data.indexPath(for: id) }
    }

    func update(ids: [ConversationList.RowID], following: Bool, request: ConversationScrollState.Request?,
                accessibilityValue: String, row: @escaping (Int) -> AnyView) {
        loadViewIfNeeded()
        layout.following = following
        if self.request != request {
            self.request = request
            requestedEdge = request?.edge
        }
        collectionView.accessibilityValue = accessibilityValue
        pending = Source(ids: ids, row: row)
        applyPending()
    }

    private func captureAnchor() {
        layout.anchor = collectionView.indexPathsForVisibleItems.sorted()
            .compactMap { path -> ConversationListLayout.Anchor? in
                guard let id = data.itemIdentifier(for: path), case .message = id,
                      let attributes = layout.layoutAttributesForItem(at: path) else { return nil }
                return ConversationListLayout.Anchor(id: id,
                                                     displacement: attributes.frame.minY - collectionView.contentOffset
                                                         .y)
            }.first
    }

    private func applyPending() {
        guard !applying, let pending, collectionView.bounds.width > 32, collectionView.bounds.height > 0 else { return }
        self.pending = nil
        captureAnchor()
        let changed = source.ids != pending.ids
        source = pending
        if changed {
            applying = true
            var snapshot = NSDiffableDataSourceSnapshot<Int, ConversationList.RowID>()
            snapshot.appendSections([0])
            snapshot.appendItems(source.ids)
            data.apply(snapshot, animatingDifferences: false) { [weak self] in
                guard let self else { return }
                applying = false
                refreshVisibleRows()
                applyPending()
            }
        } else {
            refreshVisibleRows()
        }
    }

    private func refreshVisibleRows() {
        guard active else { return }
        for path in collectionView.indexPathsForVisibleItems {
            if let cell = collectionView.cellForItem(at: path) as? ConversationListCell {
                show(cell, at: path)
            }
        }
        layout.anchor = nil
        scrollToRequestedEdge()
        publishVisibility()
    }

    private func show(_ cell: ConversationListCell, at path: IndexPath) {
        guard source.ids.indices.contains(path.item) else { return }
        cell.show(AnyView(source.row(path.item).id(source.ids[path.item])), parent: self)
    }

    override func collectionView(_: UICollectionView, willDisplay cell: UICollectionViewCell,
                                 forItemAt indexPath: IndexPath) {
        if let cell = cell as? ConversationListCell, !cell.isMounted {
            show(cell, at: indexPath)
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
        publishVisibility()
    }

    private func scrollToRequestedEdge() {
        guard !applying, !source.ids.isEmpty, let edge = requestedEdge else { return }
        requestedEdge = nil
        collectionView.scrollToItem(at: IndexPath(item: edge == .bottom ? source.ids.count - 1 : 0, section: 0),
                                    at: edge == .bottom ? .bottom : .top, animated: false)
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
        visibilityUpdate?.cancel()
        visibilityUpdate = nil
        for cell in collectionView.visibleCells {
            (cell as? ConversationListCell)?.unmount()
        }
    }

    override func scrollViewDidScroll(_: UIScrollView) {
        publishVisibility()
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
