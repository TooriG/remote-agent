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

    @MainActor static func checkCanceledLoads(_ controller: ConversationScrollObserver,
                                              scroll: UIScrollView) {
        for succeeded in [false, true] {
            controller.historyDidLayout(turnCount: 1, hasMoreHistory: true)
            let request = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
            controller.historyLoaded(request, succeeded: succeeded, turnCount: succeeded ? 1 : 2, hasMoreHistory: true)
            let prior = scroll.contentOffset
            scroll.contentSize.height += 500
            controller.historyDidLayout(turnCount: 2, hasMoreHistory: true)
            check(scroll.contentOffset == prior, "Failed and empty pages must not anchor later stream updates")
        }
        controller.historyDidLayout(turnCount: 1, hasMoreHistory: true)
        let obsolete = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
        let current = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
        controller.historyLoaded(obsolete, succeeded: true, turnCount: 2, hasMoreHistory: true)
        let prior = scroll.contentOffset
        scroll.contentSize.height += 500
        controller.historyDidLayout(turnCount: 2, hasMoreHistory: true)
        check(scroll.contentOffset == prior, "A superseded load must not restore a new request's reading position")
        controller.historyLoaded(current, succeeded: true, turnCount: 2, hasMoreHistory: true)
        check(scroll.contentOffset.y == prior.y + 500, "The current request must still restore after an old completion")
    }

    @MainActor static func checkStagedPages(_ controller: ConversationScrollObserver,
                                            scroll: UIScrollView, marker: UIView, root: UIView) {
        let stagedReadingY = marker.convert(marker.bounds, to: root).minY
        controller.historyDidLayout(turnCount: 1, hasMoreHistory: true)
        let stagedRequest = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
        controller.historyLoaded(stagedRequest, succeeded: true, turnCount: 2, hasMoreHistory: false)
        // The snapshot removes the control before the detached row projection finishes.
        marker.frame.origin.y -= 40
        scroll.contentSize.height -= 40
        controller.historyDidLayout(turnCount: 1, hasMoreHistory: false)
        marker.frame.origin.y += 1140
        scroll.contentSize.height += 1140
        controller.historyDidLayout(turnCount: 2, hasMoreHistory: false)
        check(abs(marker.convert(marker.bounds, to: root).minY - stagedReadingY) < 0.1,
              "The control disappearing before older rows render must preserve the visible message")
        let nonvisual = controller.captureReadingPosition(turnCount: 2, hasMoreHistory: false)
        controller.historyLoaded(nonvisual, succeeded: true, turnCount: 3, hasMoreHistory: false)
        controller.historyDidLayout(turnCount: 3, hasMoreHistory: false)
        let prior = scroll.contentOffset
        scroll.contentSize.height += 500
        controller.historyDidLayout(turnCount: 3, hasMoreHistory: false)
        check(scroll.contentOffset == prior, "A page without visible rows must not anchor a later stream update")
    }

    @MainActor static func checkLayouts(_ controller: ConversationScrollObserver,
                                        scroll: UIScrollView, marker: UIView, root: UIView) {
        let readingY = marker.convert(marker.bounds, to: root).minY
        for scenario in scenarios {
            controller.historyDidLayout(turnCount: 1, hasMoreHistory: true)
            let request = controller.captureReadingPosition(turnCount: 1, hasMoreHistory: true)
            controller.attach(to: scroll) // Repeated introspection must not discard the capture.
            // Layouts of the existing page must not consume the capture.
            controller.historyDidLayout(turnCount: 1, hasMoreHistory: true)
            if scenario.early {
                controller.historyLoaded(
                    request,
                    succeeded: true,
                    turnCount: scenario.turns,
                    hasMoreHistory: scenario.more
                )
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
            controller.historyDidLayout(turnCount: scenario.turns, hasMoreHistory: scenario.more)
            if !scenario.early {
                controller.historyLoaded(
                    request,
                    succeeded: true,
                    turnCount: scenario.turns,
                    hasMoreHistory: scenario.more
                )
            }
            if scenario.cancel {
                check(scroll.contentOffset == priorOffset, scenario.message)
            } else {
                check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1, scenario.message)
            }
        }
    }

    @MainActor static func main() {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 320, height: 400))
        let scroll = UIScrollView(frame: root.bounds)
        scroll.contentSize = CGSize(width: 320, height: 1000)
        root.addSubview(scroll)
        let marker = UIView(frame: CGRect(x: 0, y: 250, width: 320, height: 40))
        scroll.addSubview(marker)
        let controller = ConversationScrollObserver()
        controller.attach(to: scroll)
        scroll.contentOffset.y = 200
        checkLayouts(controller, scroll: scroll, marker: marker, root: root)
        checkStagedPages(controller, scroll: scroll, marker: marker, root: root)
        checkCanceledLoads(controller, scroll: scroll)
        controller.detach()
        print("Native history scroll tests passed")
    }
}
