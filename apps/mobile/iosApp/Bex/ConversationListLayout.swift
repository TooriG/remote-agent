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
            let width = max(1, collectionView.bounds.width - sectionInset.left - sectionInset.right)
            if estimatedItemSize.width != width {
                estimatedItemSize = CGSize(width: width, height: 100)
            }
        }
        super.prepare()
        guard let collectionView else { return }
        let insets = collectionView.adjustedContentInset
        let viewport = collectionView.bounds.height - insets.top - insets.bottom
        let height = super.collectionViewContentSize.height - sectionInset.top
        let top = max(6, viewport - height)
        if abs(top - sectionInset.top) > 0.5 {
            sectionInset.top = top
            super.prepare()
        }
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
        let context = super.invalidationContext(forPreferredLayoutAttributes: preferredAttributes,
                                                withOriginalAttributes: originalAttributes)
        guard let collectionView else { return context }
        let delta = preferredAttributes.size.height - originalAttributes.size.height
        if following {
            context.contentOffsetAdjustment.y = bottomOffset(in: collectionView, growth: delta)
                - bottomOffset(in: collectionView)
        } else {
            let insets = collectionView.adjustedContentInset
            let viewport = collectionView.bounds.height - insets.top - insets.bottom
            let height = collectionViewContentSize.height - sectionInset.top + delta
            let alignment = max(6, viewport - height) - sectionInset.top
            let above = originalAttributes.frame.maxY <= collectionView.contentOffset.y
                + collectionView.adjustedContentInset.top
            context.contentOffsetAdjustment.y = alignment + (above ? delta : 0)
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
                                                        .height - max(0, sectionInset.top - 6) - newBounds.height
                                                        + collectionView.adjustedContentInset.bottom) - collectionView
                .contentOffset.y
        }
        return context
    }

    func bottomOffset(in scroll: UIScrollView, growth: CGFloat = 0) -> CGFloat {
        let height = collectionViewContentSize.height + growth - max(0, sectionInset.top - 6)
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
        let host = host ?? UIHostingController(rootView: content)
        self.host = host
        host.rootView = content
        if host.parent == nil {
            parent.addChild(host)
            host.view.backgroundColor = .clear
            host.safeAreaRegions = []
            host.sizingOptions = .intrinsicContentSize
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
    }

    func unmount() {
        guard let host else { return }
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
        let size = contentView.systemLayoutSizeFitting(
            CGSize(width: attributes.size.width, height: UIView.layoutFittingCompressedSize.height),
            withHorizontalFittingPriority: .required, verticalFittingPriority: .fittingSizeLevel
        )
        attributes.size.height = max(1, ceil(size.height))
        return attributes
    }
}
