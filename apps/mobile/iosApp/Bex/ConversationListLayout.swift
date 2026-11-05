import SwiftUI
import UIKit

/// Native layout owns pixel anchoring, including self-sizing changes above a detached reader.
final class ConversationListLayout: UICollectionViewFlowLayout {
    struct Anchor {
        let id: ConversationList.RowID
        let displacement: CGFloat
    }

    var following = true
    var anchor: Anchor?
    var indexPath: (ConversationList.RowID) -> IndexPath? = { _ in nil }
    private var alignment: CGFloat = 0

    override init() {
        super.init()
        minimumLineSpacing = 12
        minimumInteritemSpacing = 0
        sectionInset = UIEdgeInsets(top: 6, left: 16, bottom: 6, right: 16)
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func prepare() {
        if let collectionView {
            let insets = collectionView.adjustedContentInset
            let width = max(1, collectionView.bounds.width - insets.left - insets.right
                - sectionInset.left - sectionInset.right)
            if estimatedItemSize.width != width {
                estimatedItemSize = CGSize(width: width, height: 100)
            }
        }
        super.prepare()
        guard let collectionView else { return }
        let insets = collectionView.adjustedContentInset
        let viewport = collectionView.bounds.height - insets.top - insets.bottom
        // Changing flow metrics here discards self-sizing measurements and creates a layout loop.
        // Alignment translates the measured geometry without invalidating those measurements.
        alignment = max(0, viewport - super.collectionViewContentSize.height)
    }

    override var collectionViewContentSize: CGSize {
        var size = super.collectionViewContentSize
        size.height += alignment
        return size
    }

    override func layoutAttributesForItem(at indexPath: IndexPath) -> UICollectionViewLayoutAttributes? {
        super.layoutAttributesForItem(at: indexPath).map { translated($0, by: alignment) }
    }

    override func layoutAttributesForElements(in rect: CGRect) -> [UICollectionViewLayoutAttributes]? {
        super.layoutAttributesForElements(in: rect.offsetBy(dx: 0, dy: -alignment))?
            .map { translated($0, by: alignment) }
    }

    private func translated(_ attributes: UICollectionViewLayoutAttributes,
                            by offset: CGFloat) -> UICollectionViewLayoutAttributes {
        guard let copy = attributes.copy() as? UICollectionViewLayoutAttributes else { return attributes }
        copy.frame.origin.y += offset
        return copy
    }

    override func targetContentOffset(forProposedContentOffset proposedContentOffset: CGPoint) -> CGPoint {
        guard let collectionView else { return proposedContentOffset }
        if following {
            return CGPoint(x: proposedContentOffset.x, y: bottomOffset(in: collectionView))
        }
        if let anchor, let path = indexPath(anchor.id), let attributes = layoutAttributesForItem(at: path) {
            return CGPoint(x: proposedContentOffset.x,
                           y: clamped(attributes.frame.minY - anchor.displacement, in: collectionView))
        }
        return super.targetContentOffset(forProposedContentOffset: proposedContentOffset)
    }

    override func invalidationContext(
        forPreferredLayoutAttributes preferredAttributes: UICollectionViewLayoutAttributes,
        withOriginalAttributes originalAttributes: UICollectionViewLayoutAttributes
    )
        -> UICollectionViewLayoutInvalidationContext {
        let context = super.invalidationContext(
            forPreferredLayoutAttributes: translated(preferredAttributes, by: -alignment),
            withOriginalAttributes: translated(originalAttributes, by: -alignment)
        )
        guard let collectionView else { return context }
        let delta = preferredAttributes.size.height - originalAttributes.size.height
        if following {
            context.contentOffsetAdjustment.y = bottomOffset(in: collectionView, growth: delta)
                - bottomOffset(in: collectionView)
        } else {
            let insets = collectionView.adjustedContentInset
            let viewport = collectionView.bounds.height - insets.top - insets.bottom
            let height = collectionViewContentSize.height - alignment + delta
            let shift = max(0, viewport - height) - alignment
            let above = anchor.flatMap { indexPath($0.id) }.map { originalAttributes.indexPath < $0 }
                ?? (originalAttributes.frame.maxY <= collectionView.contentOffset.y
                    + collectionView.adjustedContentInset.top)
            context.contentOffsetAdjustment.y = shift + (above ? delta : 0)
        }
        return context
    }

    override func shouldInvalidateLayout(forBoundsChange newBounds: CGRect) -> Bool {
        newBounds.size != collectionView?.bounds.size || super.shouldInvalidateLayout(forBoundsChange: newBounds)
    }

    override func invalidationContext(forBoundsChange newBounds: CGRect) -> UICollectionViewLayoutInvalidationContext {
        let context = super.invalidationContext(forBoundsChange: newBounds)
        if following, let collectionView, newBounds.height != collectionView.bounds.height {
            context.contentOffsetAdjustment.y = max(-collectionView.adjustedContentInset.top,
                                                    collectionViewContentSize
                                                        .height - alignment - newBounds.height
                                                        + collectionView.adjustedContentInset.bottom) - collectionView
                .contentOffset.y
        }
        return context
    }

    func bottomOffset(in scroll: UIScrollView, growth: CGFloat = 0) -> CGFloat {
        let height = collectionViewContentSize.height - alignment + growth
        return max(-scroll.adjustedContentInset.top,
                   height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
    }

    private func clamped(_ offset: CGFloat, in scroll: UIScrollView) -> CGFloat {
        max(-scroll.adjustedContentInset.top, min(offset, bottomOffset(in: scroll)))
    }
}

/// A real child controller preserves native selection and controller-backed SwiftUI content in recycled cells.
final class ConversationListCell: UICollectionViewCell {
    private var host: UIHostingController<AnyView>?

    var isMounted: Bool {
        host?.parent != nil
    }

    func show(_ content: AnyView, parent: UIViewController) {
        let row = AnyView(content.fixedSize(horizontal: false, vertical: true))
        let host: UIHostingController<AnyView>
        if let retained = self.host {
            host = retained
            host.rootView = row
        } else {
            host = UIHostingController(rootView: row)
            self.host = host
        }
        if host.parent == nil {
            host.safeAreaRegions = []
            host.sizingOptions = .intrinsicContentSize
            parent.addChild(host)
            host.view.backgroundColor = .clear
            host.view.translatesAutoresizingMaskIntoConstraints = false
            contentView.addSubview(host.view)
            NSLayoutConstraint.activate([
                host.view.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
                host.view.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
                host.view.topAnchor.constraint(equalTo: contentView.topAnchor),
                host.view.bottomAnchor.constraint(equalTo: contentView.bottomAnchor)
            ])
            host.didMove(toParent: parent)
        }
        // UIKit observes invalidation of the cell's content view, not the child controller's view.
        contentView.invalidateIntrinsicContentSize()
    }

    func unmount() {
        guard let host, host.parent != nil else { return }
        host.willMove(toParent: nil)
        host.view.removeFromSuperview()
        host.removeFromParent()
        host.rootView = AnyView(EmptyView())
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        unmount()
    }

    override func preferredLayoutAttributesFitting(_ layoutAttributes: UICollectionViewLayoutAttributes)
        -> UICollectionViewLayoutAttributes {
        guard let attributes = layoutAttributes.copy() as? UICollectionViewLayoutAttributes else {
            return layoutAttributes
        }
        // Recycling must not replace a measured row's size with the empty hosting view's size.
        guard let host, host.parent != nil else { return attributes }
        let proposal = CGSize(width: attributes.size.width, height: .greatestFiniteMagnitude)
        attributes.size.height = max(1, ceil(host.sizeThatFits(in: proposal).height))
        return attributes
    }
}
