import UIKit

@main
struct ConversationScrollTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    @MainActor static func nextMainQueueTurn() async {
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
    }

    @MainActor static func main() async {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 320, height: 400))
        let scroll = UIScrollView(frame: root.bounds)
        scroll.contentSize = CGSize(width: 320, height: 1000)
        root.addSubview(scroll)
        let marker = UIView(frame: CGRect(x: 0, y: 250, width: 320, height: 40))
        scroll.addSubview(marker)
        let observer = UIView(frame: .zero)
        scroll.addSubview(observer)
        let controller = ConversationScrollObserver.Coordinator()
        controller.attach(to: observer)
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 1, hasMoreHistory: true)
        scroll.contentOffset.y = 200
        let readingY = marker.convert(marker.bounds, to: root).minY
        controller.captureReadingPosition(rowCount: 1, hasMoreHistory: true)
        controller.update(loadingHistory: true, followingLatest: false, rowCount: 1, hasMoreHistory: true)
        // Model completion can reach SwiftUI before the native layout changes.
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 2, hasMoreHistory: false)
        await nextMainQueueTurn()
        marker.frame.origin.y += 1100
        scroll.contentSize.height += 1100
        await nextMainQueueTurn()
        check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1,
              "Prepending must preserve the visible marker position")

        controller.captureReadingPosition(rowCount: 2, hasMoreHistory: false)
        controller.update(loadingHistory: true, followingLatest: false, rowCount: 2, hasMoreHistory: false)
        marker.frame.origin.y += 500
        scroll.contentSize.height += 500
        scroll.contentOffset.y += 500
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 3, hasMoreHistory: false)
        await nextMainQueueTurn()
        check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1,
              "Native offset correction must not be applied twice")

        controller.captureReadingPosition(rowCount: 3, hasMoreHistory: false)
        controller.update(loadingHistory: true, followingLatest: false, rowCount: 3, hasMoreHistory: false)
        controller.cancelHistoryRestoration()
        let afterNavigation = scroll.contentOffset
        scroll.contentSize.height += 500
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 4, hasMoreHistory: false)
        await nextMainQueueTurn()
        check(scroll.contentOffset == afterNavigation, "Navigation must cancel a pending restoration")
        controller.captureReadingPosition(rowCount: 4, hasMoreHistory: false)
        controller.update(loadingHistory: true, followingLatest: false, rowCount: 4, hasMoreHistory: false)
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 4, hasMoreHistory: false)
        let afterFailedPage = scroll.contentOffset
        controller.update(loadingHistory: false, followingLatest: false, rowCount: 5, hasMoreHistory: false)
        scroll.contentSize.height += 500
        check(scroll.contentOffset == afterFailedPage, "A failed or empty page must not anchor later stream updates")
        controller.detach()
        print("Native scroll tests passed: deferred layout, native correction, navigation cancellation, failed page")
    }
}
