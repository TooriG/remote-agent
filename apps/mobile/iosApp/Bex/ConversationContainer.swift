import SwiftUI
import UIKit

struct ConversationList: UIViewControllerRepresentable {
    enum RowID: Hashable {
        case history
        case message(String)
    }

    let ids: [RowID]
    let scroll: ConversationScrollState
    let following: Bool
    let accessibilityValue: String
    let composer: AnyView
    let latest: AnyView
    let row: (Int) -> AnyView

    func makeUIViewController(context _: Context) -> ConversationContainerController {
        ConversationContainerController(scroll: scroll)
    }

    func updateUIViewController(_ controller: ConversationContainerController, context _: Context) {
        controller.updateControls(composer: composer, latest: latest)
        controller.list.update(ids: ids, following: following, accessibilityValue: accessibilityValue, row: row)
    }

    static func dismantleUIViewController(_ controller: ConversationContainerController, coordinator _: ()) {
        controller.list.stop()
    }
}

/// The list draws behind the controls; UIKit's safe area keeps its readable end above them.
final class ConversationContainerController: UIViewController {
    let list: ConversationListController
    private let composer = UIHostingController(rootView: AnyView(EmptyView()))
    private let latest = UIHostingController(rootView: AnyView(EmptyView()))

    init(scroll: ConversationScrollState) {
        list = ConversationListController(scroll: scroll)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .clear
        for host in [composer, latest] {
            host.sizingOptions = .intrinsicContentSize
            host.safeAreaRegions = []
            host.view.backgroundColor = .clear
        }
        for child in [list, composer, latest] {
            addChild(child)
            child.view.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(child.view)
            child.didMove(toParent: self)
        }
        NSLayoutConstraint.activate([
            list.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            list.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            list.view.topAnchor.constraint(equalTo: view.topAnchor),
            list.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            composer.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            composer.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            composer.view.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
            latest.view.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            latest.view.bottomAnchor.constraint(equalTo: composer.view.topAnchor, constant: -6)
        ])
    }

    func updateControls(composer: AnyView, latest: AnyView) {
        self.composer.rootView = composer
        self.latest.rootView = latest
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        let bottom = max(0, view.bounds.maxY - composer.view.frame.minY - view.safeAreaInsets.bottom)
        if list.additionalSafeAreaInsets.bottom != bottom {
            list.additionalSafeAreaInsets.bottom = bottom
        }
    }
}
