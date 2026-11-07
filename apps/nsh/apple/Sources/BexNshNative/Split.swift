#if os(macOS)
    import AppKit

    /// AppKit owns dividers, constraints, collapse and standard keyboard/accessibility behavior.
    @MainActor private final class WorkspaceSplit {
        // Background WebKit pages are owned by this native container and never
        // become extra arranged panes of the split view controller.
        let view = NSView()
        let outer = NSSplitViewController()
        let workspace = NSSplitViewController()
        let sidebar: NSSplitViewItem
        let conversation: NSSplitViewItem
        let tools: NSSplitViewItem
        private var dividerFractions: [Bool: CGFloat] = [:]

        init(sidebar: NSView, conversation: NSView, tools: NSView) {
            func controller(_ view: NSView) -> NSViewController {
                let controller = NSViewController()
                controller.view = view
                return controller
            }
            self.sidebar = NSSplitViewItem(sidebarWithViewController: controller(sidebar))
            self.conversation = NSSplitViewItem(viewController: controller(conversation))
            self.tools = NSSplitViewItem(viewController: controller(tools))
            self.sidebar.minimumThickness = 220
            self.sidebar.maximumThickness = 440
            self.sidebar.preferredThicknessFraction = 0.21
            self.sidebar.canCollapse = true
            self.sidebar.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
            self.conversation.minimumThickness = 340
            self.conversation.canCollapse = true
            self.conversation.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
            self.tools.minimumThickness = 300
            self.tools.canCollapse = true
            self.tools.collapseBehavior = .preferResizingSiblingsWithFixedSplitView
            self.tools.isCollapsed = true
            workspace.splitView.isVertical = true
            workspace.splitView.autosaveName = "BexNshWorkspaceSplit"
            workspace.addSplitViewItem(self.conversation)
            workspace.addSplitViewItem(self.tools)
            outer.splitView.isVertical = true
            outer.splitView.autosaveName = "BexNshSidebarSplit"
            outer.addSplitViewItem(self.sidebar)
            outer.addSplitViewItem(NSSplitViewItem(viewController: workspace))
            let content = outer.view
            content.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(content)
            NSLayoutConstraint.activate([
                content.leadingAnchor.constraint(equalTo: view.leadingAnchor),
                content.trailingAnchor.constraint(equalTo: view.trailingAnchor),
                content.topAnchor.constraint(equalTo: view.topAnchor),
                content.bottomAnchor.constraint(equalTo: view.bottomAnchor)
            ])
        }

        func update(sidebar: Bool, tools: Bool, layout: String) {
            let split = workspace.splitView
            let previousAxis = split.isVertical
            let previousExtent = previousAxis ? split.bounds.width : split.bounds.height
            if !conversation.isCollapsed && !self.tools.isCollapsed && previousExtent > 0 {
                let frame = conversation.viewController.view.frame
                dividerFractions[previousAxis] = (previousAxis ? frame.width : frame.height) / previousExtent
            }
            self.sidebar.isCollapsed = !sidebar
            let vertical = layout != "Bottom"
            let expanded = tools && layout == "Expanded"
            let restore = previousAxis != vertical || self.tools.isCollapsed != !tools || conversation
                .isCollapsed != expanded
            if previousAxis != vertical {
                split.isVertical = vertical
            }
            // Expand the conversation before hiding tools so the workspace always has content.
            if !expanded {
                conversation.isCollapsed = false
            }
            self.tools.isCollapsed = !tools
            if expanded {
                conversation.isCollapsed = true
            }
            if restore, tools, !expanded {
                view.layoutSubtreeIfNeeded()
                let extent = vertical ? split.bounds.width : split.bounds.height
                // AppKit owns resizing and constraints. Restore its divider after a
                // collapsed pane reopens, retaining each orientation independently.
                split.setPosition(extent * (dividerFractions[vertical] ?? 0.55), ofDividerAt: 0)
            }
        }
    }

    @_cdecl("bex_nsh_split_create")
    @MainActor public func splitCreate(
        _ sidebar: UnsafeMutableRawPointer,
        _ conversation: UnsafeMutableRawPointer,
        _ tools: UnsafeMutableRawPointer
    ) -> UnsafeMutableRawPointer {
        Unmanaged.passRetained(WorkspaceSplit(
            sidebar: Unmanaged<NSView>.fromOpaque(sidebar).takeUnretainedValue(),
            conversation: Unmanaged<NSView>.fromOpaque(conversation).takeUnretainedValue(),
            tools: Unmanaged<NSView>.fromOpaque(tools).takeUnretainedValue()
        )).toOpaque()
    }

    @_cdecl("bex_nsh_split_view")
    @MainActor public func splitView(_ handle: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
        Unmanaged.passUnretained(Unmanaged<WorkspaceSplit>.fromOpaque(handle).takeUnretainedValue().view)
            .toOpaque()
    }

    @_cdecl("bex_nsh_split_update")
    @MainActor public func splitUpdate(
        _ handle: UnsafeMutableRawPointer, _ sidebar: Bool, _ tools: Bool,
        _ layout: UnsafePointer<UInt8>, _ count: Int
    ) {
        Unmanaged<WorkspaceSplit>.fromOpaque(handle).takeUnretainedValue().update(
            sidebar: sidebar, tools: tools, layout: String(
                decoding: UnsafeBufferPointer(start: layout, count: count),
                as: UTF8.self
            )
        )
    }

    @_cdecl("bex_nsh_split_release")
    @MainActor public func splitRelease(_ handle: UnsafeMutableRawPointer) {
        Unmanaged<WorkspaceSplit>.fromOpaque(handle).release()
    }
#endif
