import SwiftUI
import UIKit

@main @MainActor
final class NavigationTests: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    private var failures = 0

    func application(_: UIApplication,
                     didFinishLaunchingWithOptions _: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task {
            await checkNavigationAppearance()
            guard failures == 0 else { exit(1) }
            print("Native navigation appearance tests passed")
            exit(0)
        }
        return true
    }

    private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            failures += 1
            return
        }
    }

    private func layout(_ controller: UIViewController) async {
        await withCheckedContinuation { continuation in
            CATransaction.begin()
            CATransaction.setCompletionBlock { continuation.resume() }
            controller.view.setNeedsLayout()
            controller.view.layoutIfNeeded()
            CATransaction.commit()
        }
        await Task.yield()
    }

    private func checkNavigationAppearance() async {
        var appearances = 0
        let root = UIHostingController(rootView: Text("Tasks").background(NavigationAppearance {
            appearances += 1
        }))
        let navigation = UINavigationController(rootViewController: root)
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 640))
        self.window = window
        window.rootViewController = navigation
        window.makeKeyAndVisible()
        await layout(navigation)
        check(appearances == 1, "The initial task list appearance must reach SwiftUI")

        navigation.pushViewController(UIViewController(), animated: false)
        await layout(navigation)
        check(appearances == 1, "Pushing a detail must not publish another list appearance")
        await withCheckedContinuation { continuation in
            navigation.popViewController(animated: true)
            check(appearances == 1, "Starting a pop must not publish the list appearance before completion")
            if let transition = navigation.transitionCoordinator {
                transition.animate(alongsideTransition: nil) { _ in continuation.resume() }
            } else {
                continuation.resume()
            }
        }
        await layout(navigation)
        check(appearances == 2, "A completed pop must publish the list appearance exactly once")

        // UIKit reverses appearance callbacks when an interactive pop is cancelled.
        let controller = NavigationAppearance.Controller()
        controller.didAppear = { appearances += 1 }
        controller.beginAppearanceTransition(true, animated: true)
        check(appearances == 2, "An interactive pop must not publish the list appearance while dragging")
        controller.beginAppearanceTransition(false, animated: true)
        controller.endAppearanceTransition()
        check(appearances == 2, "Cancelling a pop must not publish a list appearance")
    }
}
