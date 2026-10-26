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
        (early: false, native: false, cancel: false, height: 1100.0, turns: UInt64(2), more: true,
         message: "Prepending must preserve the visible marker position"),
        (early: false, native: true, cancel: false, height: 1100.0, turns: 2, more: true,
         message: "Native offset correction must not be applied twice"),
        (early: true, native: false, cancel: false, height: 1100.0, turns: 2, more: true,
         message: "History completion before layout must retain the reading position"),
        (early: false, native: false, cancel: false, height: -40.0, turns: 1, more: false,
         message: "Removing the last page control must preserve the visible message"),
        (early: false, native: false, cancel: true, height: 1100.0, turns: 2, more: true,
         message: "Navigation must cancel a pending restoration")
    ]

    @MainActor static func flushMainQueue() async {
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
    }

    @MainActor static func checkCanceledLoads(_ controller: ConversationScrollObserver.Coordinator,
                                              scroll: UIScrollView) {
        for succeeded in [false, true] {
            let request = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
            controller.historyLoaded(request, succeeded: succeeded, turnCount: succeeded ? 1 : 2, hasMoreHistory: true)
            let prior = scroll.contentOffset
            scroll.contentSize.height += 500
            check(scroll.contentOffset == prior, "Failed and empty pages must not anchor later stream updates")
        }
        let obsolete = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
        let current = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
        controller.historyLoaded(obsolete, succeeded: true, turnCount: 2, hasMoreHistory: true)
        let prior = scroll.contentOffset
        scroll.contentSize.height += 500
        check(scroll.contentOffset == prior, "A superseded load must not restore a new request's reading position")
        controller.historyLoaded(current, succeeded: true, turnCount: 2, hasMoreHistory: true)
        check(scroll.contentOffset.y == prior.y + 500, "The current request must still restore after an old completion")
    }

    @MainActor static func main() async {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 320, height: 400))
        let scroll = UIScrollView(frame: root.bounds)
        scroll.contentSize = CGSize(width: 320, height: 1000)
        root.addSubview(scroll)
        let marker = UIView(frame: CGRect(x: 0, y: 250, width: 320, height: 40))
        scroll.addSubview(marker)
        let controller = ConversationScrollObserver.Coordinator()
        controller.attach(to: marker)
        scroll.contentOffset.y = 200
        let readingY = marker.convert(marker.bounds, to: root).minY
        for scenario in scenarios {
            let request = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
            await flushMainQueue() // Unrelated idle updates must not consume the capture.
            if scenario.early {
                controller.historyLoaded(
                    request,
                    succeeded: true,
                    turnCount: scenario.turns,
                    hasMoreHistory: scenario.more
                )
                await flushMainQueue()
            }
            if scenario.cancel {
                controller.cancelHistoryRestoration()
            }
            let priorOffset = scroll.contentOffset
            marker.frame.origin.y += scenario.height
            scroll.contentSize.height += scenario.height
            if scenario.native {
                scroll.contentOffset.y += scenario.height
            }
            if !scenario.early {
                controller.historyLoaded(
                    request,
                    succeeded: true,
                    turnCount: scenario.turns,
                    hasMoreHistory: scenario.more
                )
            }
            await flushMainQueue()
            if scenario.cancel {
                check(scroll.contentOffset == priorOffset, scenario.message)
            } else {
                check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1, scenario.message)
            }
        }
        checkCanceledLoads(controller, scroll: scroll)
        controller.detach()
        print("Native scroll tests passed: layout order, correction, cancellation, empty pages, superseded loads")
    }
}
