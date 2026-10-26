import UIKit

@main
struct ConversationScrollTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    @MainActor static func apply(_ point: CGPoint?, to scroll: UIScrollView) {
        if let point {
            // ScrollPosition applies its point relative to the inset content origin.
            let insets = scroll.adjustedContentInset
            scroll.setContentOffset(CGPoint(x: point.x - insets.left, y: point.y - insets.top), animated: false)
        }
    }

    /// Model SwiftUI publishing geometry and applying the returned position to its view.
    @MainActor static func render(_ controller: ConversationScrollObserver, scroll: UIScrollView,
                                  turns: UInt64, more: Bool) {
        apply(controller.historyDidLayout(turnCount: turns, hasMoreHistory: more,
                                          contentHeight: scroll.contentSize.height,
                                          contentOffset: scroll.contentOffset,
                                          scrollOrigin: CGPoint(x: scroll.adjustedContentInset.left,
                                                                y: scroll.adjustedContentInset.top)), to: scroll)
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
            render(controller, scroll: scroll, turns: 1, more: true)
            let request = controller.captureReadingPosition()
            apply(
                controller
                    .historyLoaded(request, succeeded: succeeded, turnCount: succeeded ? 1 : 2, hasMoreHistory: true),
                to: scroll
            )
            let prior = scroll.contentOffset
            scroll.contentSize.height += 500
            render(controller, scroll: scroll, turns: 2, more: true)
            check(scroll.contentOffset == prior, "Failed and empty pages must not anchor later stream updates")
        }
        render(controller, scroll: scroll, turns: 1, more: true)
        let obsolete = controller.captureReadingPosition()
        let current = controller.captureReadingPosition()
        apply(controller.historyLoaded(obsolete, succeeded: true, turnCount: 2, hasMoreHistory: true), to: scroll)
        let prior = scroll.contentOffset
        scroll.contentSize.height += 500
        render(controller, scroll: scroll, turns: 2, more: true)
        check(scroll.contentOffset == prior, "A superseded load must not restore a new request's reading position")
        apply(controller.historyLoaded(current, succeeded: true, turnCount: 2, hasMoreHistory: true), to: scroll)
        check(scroll.contentOffset.y == prior.y + 500, "The current request must still restore after an old completion")
    }

    @MainActor static func checkStagedPages(_ controller: ConversationScrollObserver,
                                            scroll: UIScrollView, marker: UIView, root: UIView) {
        let stagedReadingY = marker.convert(marker.bounds, to: root).minY
        render(controller, scroll: scroll, turns: 1, more: true)
        let stagedRequest = controller.captureReadingPosition()
        apply(controller.historyLoaded(stagedRequest, succeeded: true, turnCount: 2, hasMoreHistory: false), to: scroll)
        // The snapshot removes the control before the detached row projection finishes.
        marker.frame.origin.y -= 40
        scroll.contentSize.height -= 40
        render(controller, scroll: scroll, turns: 1, more: false)
        marker.frame.origin.y += 1140
        scroll.contentSize.height += 1140
        render(controller, scroll: scroll, turns: 2, more: false)
        check(abs(marker.convert(marker.bounds, to: root).minY - stagedReadingY) < 0.1,
              "The control disappearing before older rows render must preserve the visible message")
        let nonvisual = controller.captureReadingPosition()
        apply(controller.historyLoaded(nonvisual, succeeded: true, turnCount: 3, hasMoreHistory: false), to: scroll)
        render(controller, scroll: scroll, turns: 3, more: false)
        let prior = scroll.contentOffset
        scroll.contentSize.height += 500
        render(controller, scroll: scroll, turns: 3, more: false)
        check(scroll.contentOffset == prior, "A page without visible rows must not anchor a later stream update")
    }

    @MainActor static func checkLayouts(_ controller: ConversationScrollObserver,
                                        scroll: UIScrollView, marker: UIView, root: UIView) {
        let readingY = marker.convert(marker.bounds, to: root).minY
        for scenario in scenarios {
            render(controller, scroll: scroll, turns: 1, more: true)
            let request = controller.captureReadingPosition()
            controller.attach(to: scroll) // Repeated introspection must not discard the capture.
            // Layouts of the existing page must not consume the capture.
            render(controller, scroll: scroll, turns: 1, more: true)
            if scenario.early {
                apply(controller.historyLoaded(request, succeeded: true, turnCount: scenario.turns,
                                               hasMoreHistory: scenario.more), to: scroll)
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
            render(controller, scroll: scroll, turns: scenario.turns, more: scenario.more)
            if !scenario.early {
                apply(controller.historyLoaded(request, succeeded: true, turnCount: scenario.turns,
                                               hasMoreHistory: scenario.more), to: scroll)
            }
            if scenario.cancel {
                check(scroll.contentOffset == priorOffset, scenario.message)
            } else {
                check(abs(marker.convert(marker.bounds, to: root).minY - readingY) < 0.1, scenario.message)
            }
        }
    }

    @MainActor static func checkDelayedNativeGeometry(_ controller: ConversationScrollObserver,
                                                      scroll: UIScrollView) {
        let baselineHeight = scroll.contentSize.height
        apply(controller.historyDidLayout(turnCount: 1, hasMoreHistory: true,
                                          contentHeight: baselineHeight, contentOffset: scroll.contentOffset,
                                          scrollOrigin: .zero),
              to: scroll)
        let request = controller.captureReadingPosition()
        apply(controller.historyLoaded(request, succeeded: true, turnCount: 2, hasMoreHistory: false), to: scroll)
        let prior = scroll.contentOffset
        let height = baselineHeight + 1100
        let restored = controller.historyDidLayout(
            turnCount: 2,
            hasMoreHistory: false,
            contentHeight: height,
            contentOffset: scroll.contentOffset,
            scrollOrigin: .zero
        )
        check(restored == CGPoint(x: prior.x, y: prior.y + 1100),
              "Use the published geometry before the native content size updates")
        check(scroll.contentOffset == prior, "Only the screen may apply the returned reading position")
    }

    @MainActor static func checkScrollingDuringLoad(_ controller: ConversationScrollObserver,
                                                    scroll: UIScrollView) {
        render(controller, scroll: scroll, turns: 1, more: true)
        let request = controller.captureReadingPosition()
        scroll.contentOffset.y += 40
        render(controller, scroll: scroll, turns: 1, more: true)
        let readingOffset = scroll.contentOffset.y
        apply(controller.historyLoaded(request, succeeded: true, turnCount: 2, hasMoreHistory: true), to: scroll)
        scroll.contentSize.height += 500
        render(controller, scroll: scroll, turns: 2, more: true)
        check(scroll.contentOffset.y == readingOffset + 500,
              "Keep the user's scrolling while the history request is pending")
    }

    @MainActor static func checkChangingViewportInsets() {
        let root = UIView(frame: CGRect(x: 0, y: 0, width: 402, height: 874))
        let scroll = UIScrollView(frame: root.bounds)
        root.addSubview(scroll)
        scroll.contentSize = CGSize(width: 402, height: 433)
        scroll.contentInset = UIEdgeInsets(top: 257, left: 20, bottom: 184, right: 0)
        scroll.contentOffset = CGPoint(x: -20, y: -257)
        let marker = UIView(frame: CGRect(x: 50, y: 300, width: 100, height: 40))
        scroll.addSubview(marker)
        let controller = ConversationScrollObserver()
        render(controller, scroll: scroll, turns: 5, more: true)
        let request = controller.captureReadingPosition()
        let readingPosition = marker.convert(marker.bounds, to: root).origin
        apply(controller.historyLoaded(request, succeeded: true, turnCount: 10, hasMoreHistory: false), to: scroll)
        marker.frame.origin.y += 1080
        scroll.contentSize.height += 1080
        scroll.contentInset = UIEdgeInsets(top: 116, left: 40, bottom: 184, right: 0)
        render(controller, scroll: scroll, turns: 10, more: false)
        let restored = marker.convert(marker.bounds, to: root).origin
        check(abs(restored.x - readingPosition.x) < 0.1 && abs(restored.y - readingPosition.y) < 0.1,
              "Use the completed page's insets when converting geometry into a ScrollPosition point")
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
        check(controller.captureReadingPosition() == nil, "Capture requires a published SwiftUI geometry")
        checkLayouts(controller, scroll: scroll, marker: marker, root: root)
        checkStagedPages(controller, scroll: scroll, marker: marker, root: root)
        checkDelayedNativeGeometry(controller, scroll: scroll)
        checkScrollingDuringLoad(controller, scroll: scroll)
        checkCanceledLoads(controller, scroll: scroll)
        checkChangingViewportInsets()
        controller.detach()
        print("Native history scroll tests passed")
    }
}
