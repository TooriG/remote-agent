import AgentCore
import SwiftTerm
import SwiftUI

struct TerminalScreen: View {
    @ObservedObject var model: AppViewModel
    let cwd: String
    let handle: String

    var body: some View {
        VStack(spacing: 0) {
            let terminal = model.snapshot.terminalView(handle: handle)
            if terminal?.loading ?? true {
                ProgressView().accessibilityIdentifier("terminal.loading")
            } else if let status = terminal?.status {
                AppText(status).font(.caption).foregroundStyle(.secondary)
            }
            NativeTerminalView(session: model.session, handle: handle, cwd: cwd, terminal: terminal)
        }
    }
}

private struct NativeTerminalView: UIViewRepresentable {
    let session: ClientSession?
    let handle: String
    let cwd: String
    let terminal: AgentCore.TerminalView?

    func makeCoordinator() -> Coordinator {
        Coordinator(session: session, handle: handle, cwd: cwd)
    }

    func makeUIView(context: Context) -> SwiftTerm.TerminalView {
        let view = HostTerminalView(frame: .zero)
        view.terminalDelegate = context.coordinator
        view.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        view.accessibilityIdentifier = "terminal.screen"
        return view
    }

    static func dismantleUIView(_ view: SwiftTerm.TerminalView, coordinator: Coordinator) {
        view.terminalDelegate = nil
        coordinator.dismantled = true
        if coordinator.started {
            coordinator.dispatch(.detachTerminal(DetachTerminal(handle: coordinator.handle)))
        }
    }

    func updateUIView(_ view: SwiftTerm.TerminalView, context: Context) {
        let coordinator = context.coordinator
        guard let terminal else { return }
        view.accessibilityValue = terminal.acceptsInput && !terminal.loading ? "入力可能" : nil
        let previousSequence = coordinator.sequence
        for chunk in terminal.output where chunk.sequence > coordinator.sequence {
            if let size = chunk.resetSize {
                coordinator.restoring = true
                view.resize(cols: Int(size.cols), rows: Int(size.rows))
            }
            view.feed(byteArray: Array(chunk.data)[...])
            coordinator.restoring = false
            coordinator.sequence = chunk.sequence
        }
        if coordinator.sequence > previousSequence {
            coordinator.dispatch(.acknowledgeTerminal(handle: handle, sequence: coordinator.sequence))
        }
    }

    @MainActor final class Coordinator: NSObject, TerminalViewDelegate {
        // The session owns this terminal; its observer publishes UI changes after
        // UIKit updates. Profile changes must not retarget existing callbacks.
        let session: ClientSession?
        let handle: String
        var sequence: UInt64 = 0
        var restoring = false
        var started = false
        var dismantled = false
        let cwd: String
        init(session: ClientSession?, handle: String, cwd: String) {
            self.session = session; self.handle = handle; self.cwd = cwd
        }

        func dispatch(_ intent: Intent) {
            guard let session, let receipt = try? session.dispatch(intent: intent) else { return }
            // Preserve input order and drain tracked receipts without publishing
            // an AppViewModel snapshot from a UIKit lifecycle callback.
            Task { _ = try? await receipt.wait() }
        }

        func send(source _: SwiftTerm.TerminalView, data: ArraySlice<UInt8>) {
            guard !dismantled, let session,
                  session.snapshot().terminalView(handle: handle)?.acceptsInput == true else { return }
            dispatch(.writeTerminal(WriteTerminal(handle: handle, data: Data(data))))
        }

        func sizeChanged(source _: SwiftTerm.TerminalView, newCols: Int, newRows: Int) {
            guard !dismantled, !restoring, newCols > 0, newRows > 0 else { return }
            let size = TerminalSize(
                cols: UInt16(clamping: min(newCols, 500)),
                rows: UInt16(clamping: min(newRows, 250))
            )
            Task { @MainActor in
                guard !dismantled, let session else { return }
                if !started {
                    started = true
                    dispatch(.startTerminal(StartTerminal(handle: handle, cwd: cwd, size: size)))
                } else if session.snapshot().terminalView(handle: handle)?.acceptsInput == true {
                    dispatch(.resizeTerminal(ResizeTerminal(handle: handle, size: size)))
                }
            }
        }

        func setTerminalTitle(source _: SwiftTerm.TerminalView, title _: String) {}
        func hostCurrentDirectoryUpdate(source _: SwiftTerm.TerminalView, directory _: String?) {}
        func scrolled(source _: SwiftTerm.TerminalView, position _: Double) {}
        func rangeChanged(source _: SwiftTerm.TerminalView, startY _: Int, endY _: Int) {}
        func requestOpenLink(source _: SwiftTerm.TerminalView, link: String, params _: [String: String]) {
            guard let url = URL(string: link),
                  ["https", "http"].contains(url.scheme?.lowercased() ?? "") else { return }
            UIApplication.shared.open(url)
        }
    }
}

/// Emulator responses are produced by the Host; SwiftTerm still handles native user input.
private final class HostTerminalView: SwiftTerm.TerminalView {
    @TaskLocal private nonisolated static var isPasting = false

    override func paste(_ sender: Any?) {
        // SwiftTerm sends paste bytes synchronously through the emulator delegate.
        // Scope forwarding to this call, without enabling replies from its parser thread.
        Self.$isPasting.withValue(true) { super.paste(sender) }
    }

    override nonisolated func send(source: SwiftTerm.Terminal, data: ArraySlice<UInt8>) {
        guard Self.isPasting else { return }
        super.send(source: source, data: data)
    }
}
