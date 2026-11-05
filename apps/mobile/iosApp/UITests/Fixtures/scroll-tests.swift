import SwiftUI
import UIKit

@main @MainActor
final class ConversationScrollTests: UIResponder, UIApplicationDelegate {
    private var window: UIWindow?

    func application(_: UIApplication,
                     didFinishLaunchingWithOptions _: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task {
            await checkRecyclingAndAnchoring()
            await checkFollowingTallRows()
            await checkShortHistory()
            print("Native conversation recycling and scroll tests passed")
            exit(0)
        }
        return true
    }

    private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    private func mount(_ controller: ConversationListController) {
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 640))
        self.window = window
        window.rootViewController = controller
        window.makeKeyAndVisible()
    }

    private func layout(_ controller: ConversationListController) async {
        // Complete the actual UIKit transaction rather than simulating content heights or retrying assertions.
        await withCheckedContinuation { continuation in
            CATransaction.begin()
            CATransaction.setCompletionBlock { continuation.resume() }
            controller.view.setNeedsLayout()
            controller.view.layoutIfNeeded()
            CATransaction.commit()
        }
        await Task.yield()
    }

    private func position(_ item: Int, in controller: ConversationListController) -> CGFloat {
        let view = controller.collectionView!
        guard let attributes = view.layoutAttributesForItem(at: IndexPath(item: item, section: 0)) else {
            preconditionFailure("Missing row geometry")
        }
        return attributes.frame.minY - view.contentOffset.y
    }

    private func checkRecyclingAndAnchoring() async {
        let scroll = ConversationScrollState()
        let controller = ConversationListController(scroll: scroll)
        mount(controller)
        let ids = (0 ..< 10000).map { ConversationList.RowID.message(String($0)) }
        var built = 0
        var heights = [String: CGFloat]()
        func update(_ rows: [ConversationList.RowID]) {
            let sizes = heights
            controller.update(ids: rows, following: scroll.following, request: scroll.request,
                              accessibilityValue: "items=\(rows.count)") { index in
                built += 1
                let id = rows[index]
                let height: CGFloat = if case let .message(key) = id {
                    sizes[key] ?? 72
                } else {
                    40
                }
                return AnyView(Text(String(describing: id)).frame(maxWidth: .infinity).frame(height: height))
            }
        }
        update(ids)
        await layout(controller)
        let view = controller.collectionView!
        check(built < 120, "Ten thousand rows must materialize only the viewport, including initial positioning")
        check(view.indexPathsForVisibleItems.contains(IndexPath(item: 9999, section: 0)), "Open at the latest row")
        check(controller.children.count < 40, "Recycling must keep the hosting-controller hierarchy bounded")

        scroll.goTo(.top)
        update(ids)
        await layout(controller)
        check(view.indexPathsForVisibleItems.contains(IndexPath(item: 0, section: 0)), "Top navigation must detach")
        let reading = position(0, in: controller)
        update(ids) // Failed, empty, and still-pending history requests do not mutate the list.
        await layout(controller)
        check(abs(position(0, in: controller) - reading) < 1, "An unchanged page must not move the reader")
        view.scrollToItem(at: IndexPath(item: 500, section: 0), at: .top, animated: false)
        await layout(controller)
        let anchor = position(500, in: controller)
        let older = (0 ..< 50).map { ConversationList.RowID.message("older-\($0)") }
        let prepended = older + ids
        update(prepended)
        await layout(controller)
        check(abs(position(550, in: controller) - anchor) < 1, "A prepend must preserve the same row and pixel")
        heights["9999"] = 1200
        update(prepended)
        await layout(controller)
        check(abs(position(550, in: controller) - anchor) < 1, "Detached readers must ignore tail growth")
        check(built < 300, "Prepending must not eagerly create the history")
        check(controller.children.count < 40, "Offscreen cells must detach their child controllers")
    }

    private func checkFollowingTallRows() async {
        let scroll = ConversationScrollState()
        let controller = ConversationListController(scroll: scroll)
        mount(controller)
        let ids = [ConversationList.RowID.message("first"), .message("last")]
        func update(_ height: CGFloat) {
            controller.update(ids: ids, following: scroll.following, request: scroll.request,
                              accessibilityValue: "") { index in
                AnyView(Text("Long message").frame(height: index == 0 ? 72 : height))
            }
        }
        update(1200)
        await layout(controller)
        let view = controller.collectionView!
        let bottom = view.contentOffset.y + view.bounds.height - view.adjustedContentInset.bottom
        let last = view.layoutAttributesForItem(at: IndexPath(item: 1, section: 0))!
        check(abs(bottom - last.frame.maxY - 6) < 1, "Opening must reveal the end of a very tall row")
        update(1600)
        await layout(controller)
        let grown = view.layoutAttributesForItem(at: IndexPath(item: 1, section: 0))!
        check(abs(view.contentOffset.y + view.bounds.height - view.adjustedContentInset.bottom
                - grown.frame.maxY - 6) < 1, "Following must track streamed self-sizing growth")
        check(controller.scrollViewShouldScrollToTop(view) && !scroll.following,
              "Status-bar navigation must detach before UIKit scrolls")
    }

    private func checkShortHistory() async {
        let scroll = ConversationScrollState()
        let controller = ConversationListController(scroll: scroll)
        mount(controller)
        let latest = ConversationList.RowID.message("latest")
        func update(_ ids: [ConversationList.RowID]) {
            controller.update(ids: ids, following: scroll.following, request: scroll.request,
                              accessibilityValue: "") { _ in AnyView(Text("Message").frame(height: 60)) }
        }
        update([.history, latest])
        await layout(controller)
        let reading = position(1, in: controller)
        scroll.following = false
        // The history control can disappear before its asynchronous row projection completes.
        update([latest])
        await layout(controller)
        check(abs(position(0, in: controller) - reading) < 1, "Removing a history control must preserve the message")
        let ids = (0 ..< 50).map { ConversationList.RowID.message("old-\($0)") } + [latest]
        update(ids)
        await layout(controller)
        check(abs(position(50, in: controller) - reading) < 1, "A staged prepend must preserve bottom-aligned content")
        let view = controller.collectionView!
        let before = view.contentOffset
        update(ids)
        await layout(controller)
        check(view.contentOffset == before, "A nonvisual page must not anchor a later unrelated update")
        let replacement = ConversationListController(scroll: ConversationScrollState())
        mount(replacement)
        replacement.update(ids: [.message("other")], following: true, request: nil, accessibilityValue: "") { _ in
            AnyView(Text("Other conversation").frame(height: 60))
        }
        await layout(replacement)
        check(replacement.collectionView.numberOfItems(inSection: 0) == 1,
              "Navigation must discard the previous conversation's pending source and anchors")
    }
}
