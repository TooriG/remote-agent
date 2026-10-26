import UIKit

@main
struct ConversationScrollTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
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
        controller.update(loadingHistory: false, followingLatest: false)
        scroll.contentOffset.y = 200
        let readingY = marker.convert(marker.bounds, to: root).minY
        controller.captureReadingPosition()
        controller.update(loadingHistory: true, followingLatest: false)
        marker.frame.origin.y += 1100
        scroll.contentSize.height += 1100
        controller.update(loadingHistory: false, followingLatest: false)
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
        check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1,
              "Prepending must preserve the visible marker position")

        controller.captureReadingPosition()
        controller.update(loadingHistory: true, followingLatest: false)
        marker.frame.origin.y += 500
        scroll.contentSize.height += 500
        scroll.contentOffset.y += 500
        controller.update(loadingHistory: false, followingLatest: false)
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
        check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1,
              "Native offset correction must not be applied twice")

        controller.captureReadingPosition()
        controller.update(loadingHistory: true, followingLatest: false)
        controller.cancelHistoryRestoration()
        let afterNavigation = scroll.contentOffset
        scroll.contentSize.height += 500
        controller.update(loadingHistory: false, followingLatest: false)
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
        check(scroll.contentOffset == afterNavigation, "Navigation must cancel a pending restoration")
        controller.detach()
        print("Native scroll tests passed: prepend, native correction, navigation cancellation")
    }
}
