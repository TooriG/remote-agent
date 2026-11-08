#if os(macOS)
    import AppKit

    /// Fixture-only timing. The normal app neither records message text nor
    /// performs probe I/O. Times use Foundation's reference date on both sides.
    @MainActor enum RenderProbe {
        static let enabled = ProcessInfo.processInfo.environment["BEX_ACCEPTANCE_OUTPUT"] != nil
        private static let logging = enabled && ProcessInfo.processInfo.environment["BEX_RENDER_TRACE"] == "1"

        static func record(_ stage: String, _ view: NSTextView, painted: String = "") {
            guard logging else { return }
            let record: [String: Any] = [
                "stage": stage, "at": Date.timeIntervalSinceReferenceDate,
                "view": String(describing: Unmanaged.passUnretained(view).toOpaque()),
                "text": String(view.string.prefix(100)), "length": view.string.utf16.count,
                "width": view.bounds.width, "height": view.bounds.height,
                "visible": !view.visibleRect.isEmpty,
                "constraint": (view as? MessageTextView)?.document.maxWidth ?? 0,
                "painted": String(painted.prefix(200)),
                "clipY": view.visibleRect.origin.y, "clipHeight": view.visibleRect.height,
            ]
            guard let data = try? JSONSerialization.data(withJSONObject: record),
                  let line = String(data: data, encoding: .utf8) else { return }
            FileHandle.standardError.write(Data("render-probe \(line)\n".utf8))
        }
    }
#endif
