import UIKit

@main
struct ConversationScrollTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    static let scenarios = [
        (loading: [true], nativeCorrection: false, cancel: false,
         message: "Prepending must preserve the visible marker position"),
        (loading: [true], nativeCorrection: true, cancel: false,
         message: "Native offset correction must not be applied twice"),
        (loading: [true, false], nativeCorrection: false, cancel: false,
         message: "History completion before layout must retain the reading position"),
        (loading: [false, true], nativeCorrection: false, cancel: false,
         message: "An intermediate idle update must not consume the reading position"),
        (loading: [true], nativeCorrection: false, cancel: true,
         message: "Navigation must cancel a pending restoration")
    ]

    static func flushMainQueue() async {
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
        controller.update(loadingHistory: false, followingLatest: false)
        scroll.contentOffset.y = 200
        let readingY = marker.convert(marker.bounds, to: root).minY
        for scenario in scenarios {
            controller.captureReadingPosition()
            for loading in scenario.loading {
                controller.update(loadingHistory: loading, followingLatest: false)
                await flushMainQueue()
            }
            if scenario.cancel {
                controller.cancelHistoryRestoration()
            }
            let priorOffset = scroll.contentOffset
            marker.frame.origin.y += 1100
            scroll.contentSize.height += 1100
            if scenario.nativeCorrection {
                scroll.contentOffset.y += 1100
            }
            controller.update(loadingHistory: false, followingLatest: false)
            await flushMainQueue()
            if scenario.cancel {
                check(scroll.contentOffset == priorOffset, scenario.message)
            } else {
                check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1, scenario.message)
            }
        }
        controller.detach()
        print("Native scroll tests passed: prepend, correction, delayed layout, idle updates, cancellation")
    }
}
