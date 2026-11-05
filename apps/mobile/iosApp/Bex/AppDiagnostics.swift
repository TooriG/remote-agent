import Foundation
import OSLog
import SwiftUI
import UIKit

/// Independent of Host/session restoration, so a launch that never connects still leaves evidence.
/// Only fixed milestones and numeric measurements enter this bounded, device-local journal.
final class AppDiagnostics: @unchecked Sendable {
    enum Event: String, Encodable {
        case launchStarted, modelInitializing, profilesLoading, profilesLoaded, profilesLoadFailed, modelReady
        case rootAppeared, rootDisappeared, windowAttached, windowDetached, rootLaidOut, displayRefresh
        case profilesScreen, pairingScreen, threadsScreen, conversationScreen
        case sceneActive, sceneInactive, sceneBackground
        case restorationStarted, restorationReady, connectionStarted, connectionReady, connectionFailed,
             connectionCancelled
        case mainThreadStalled, mainThreadRecovered
    }

    static let shared = AppDiagnostics(directory: (
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
    ).appendingPathComponent("diagnostics", isDirectory: true))

    private struct Record: Encodable {
        let timestampMs: Int64
        let uptimeMs: Int64
        let launch: UUID
        let pid: Int32
        let build: String
        let revision: String
        let event: Event
        let value: Int
    }

    private let directory: URL
    private let fileBytes: Int
    private let launch = UUID()
    private let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0"
    private let revision = Bundle.main.object(forInfoDictionaryKey: "BEXBuildRevision") as? String ?? "development"
    private let queue = DispatchQueue(label: "bex.app-diagnostics", qos: .utility)
    private let logger = Logger(subsystem: Bundle.main.bundleIdentifier ?? "bex", category: "AppDiagnostics")
    private var timer: DispatchSourceTimer?
    private var probe = MainThreadProbe()

    init(directory: URL, fileBytes: Int = 128 * 1024) {
        self.directory = directory
        self.fileBytes = fileBytes
    }

    deinit { timer?.cancel() }

    func start() {
        record(.launchStarted)
        queue.async {
            guard self.timer == nil else { return }
            let timer = DispatchSource.makeTimerSource(queue: self.queue)
            timer.schedule(deadline: .now() + 1, repeating: 1, leeway: .milliseconds(100))
            timer.setEventHandler { [weak self] in self?.checkMainThread() }
            self.timer = timer
            timer.resume()
        }
    }

    func record(_ event: Event, value: Int = 0) {
        let record = Record(timestampMs: Int64(Date().timeIntervalSince1970 * 1000),
                            uptimeMs: Int64(ProcessInfo.processInfo.systemUptime * 1000),
                            launch: launch, pid: ProcessInfo.processInfo.processIdentifier,
                            build: build, revision: revision, event: event, value: value)
        queue.async { self.append(record) }
    }

    func scene(_ phase: ScenePhase) {
        record(phase == .active ? .sceneActive : phase == .background ? .sceneBackground : .sceneInactive)
        queue.async {
            self.probe.setActive(phase == .active)
        }
    }

    func flush() async {
        await withCheckedContinuation { continuation in
            queue.async { continuation.resume() }
        }
    }

    private func checkMainThread() {
        let sentAt = ProcessInfo.processInfo.systemUptime
        switch probe.poll(at: sentAt) {
        case .ping:
            DispatchQueue.main.async {
                let repliedAt = ProcessInfo.processInfo.systemUptime
                self.queue.async {
                    if let delay = self.probe.reply(sentAt: sentAt, at: repliedAt) {
                        self.record(.mainThreadRecovered, value: delay)
                    }
                }
            }
        case let .stalled(delay): record(.mainThreadStalled, value: delay)
        case nil: break
        }
    }

    private func append(_ record: Record) {
        do {
            let manager = FileManager.default
            try manager.createDirectory(at: directory, withIntermediateDirectories: true,
                                        attributes: [.posixPermissions: 0o700])
            var directory = directory
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? directory.setResourceValues(values)
            let file = directory.appendingPathComponent("app.jsonl")
            var bytes = try JSONEncoder().encode(record)
            bytes.append(0x0A)
            let size = (try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            // Reserve a separator in case a previous write was interrupted mid-record.
            if size + bytes.count + 1 > fileBytes {
                let oldest = directory.appendingPathComponent("app.jsonl.2")
                if manager.fileExists(atPath: oldest.path) {
                    try manager.removeItem(at: oldest)
                }
                let previous = directory.appendingPathComponent("app.jsonl.1")
                if manager.fileExists(atPath: previous.path) {
                    try manager.moveItem(at: previous, to: oldest)
                }
                if manager.fileExists(atPath: file.path) {
                    try manager.moveItem(at: file, to: previous)
                }
            }
            if !manager.fileExists(atPath: file.path) {
                guard manager.createFile(atPath: file.path, contents: nil, attributes: [.posixPermissions: 0o600])
                else {
                    throw CocoaError(.fileWriteUnknown)
                }
            }
            let output = try FileHandle(forUpdating: file)
            defer { try? output.close() }
            let end = try output.seekToEnd()
            if end > 0 {
                try output.seek(toOffset: end - 1)
                let lastByte = try output.read(upToCount: 1)
                try output.seekToEnd()
                if lastByte != Data([0x0A]) {
                    try output.write(contentsOf: Data([0x0A]))
                }
            }
            try output.write(contentsOf: bytes)
        } catch {
            // Logging failure must neither stop launch nor disclose an arbitrary error message/path.
            logger.error("App diagnostic write failed: code=\((error as NSError).code, privacy: .public)")
        }
    }
}

/// One report per outstanding main-queue probe, followed by its recovery time.
/// Reset on inactive/background so normal suspension is never counted as a hang.
struct MainThreadProbe {
    private static let stallInterval: TimeInterval = 2
    enum Action: Equatable {
        case ping, stalled(Int)
    }

    private var active = true
    private var sentAt: TimeInterval?
    private var reported = false

    mutating func setActive(_ active: Bool) {
        self.active = active
        sentAt = nil
        reported = false
    }

    mutating func poll(at now: TimeInterval) -> Action? {
        guard active else { return nil }
        guard let sentAt else {
            sentAt = now
            return .ping
        }
        guard !reported, now - sentAt >= Self.stallInterval else { return nil }
        reported = true
        return .stalled(Int((now - sentAt) * 1000))
    }

    mutating func reply(sentAt: TimeInterval, at now: TimeInterval) -> Int? {
        guard self.sentAt == sentAt else { return nil }
        defer { self.sentAt = nil; reported = false }
        return reported || now - sentAt >= Self.stallInterval ? Int((now - sentAt) * 1000) : nil
    }
}

struct AppDiagnosticView: UIViewRepresentable {
    let active: Bool

    func makeUIView(context _: Context) -> AppDiagnosticUIView {
        AppDiagnosticUIView(diagnostics: .shared)
    }

    func updateUIView(_ uiView: AppDiagnosticUIView, context _: Context) {
        uiView.updateActive(active)
    }
}

/// These are UIKit/layout/display-clock milestones, not proof of GPU presentation or visible pixels.
final class AppDiagnosticUIView: UIView {
    private let diagnostics: AppDiagnostics
    private var active = false
    private var laidOut = false
    private var displayLink: CADisplayLink?

    init(diagnostics: AppDiagnostics) {
        self.diagnostics = diagnostics
        super.init(frame: .zero)
        isUserInteractionEnabled = false
        backgroundColor = .clear
        isOpaque = false
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        nil
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        laidOut = false
        diagnostics.record(window == nil ? .windowDetached : .windowAttached)
        observeDisplay()
    }

    func updateActive(_ active: Bool) {
        guard self.active != active else { return }
        self.active = active
        laidOut = false
        setNeedsLayout()
        observeDisplay()
    }

    private func observeDisplay() {
        displayLink?.invalidate()
        displayLink = nil
        if window != nil, active {
            let link = CADisplayLink(target: self, selector: #selector(displayRefreshed))
            displayLink = link
            link.add(to: .main, forMode: .common)
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil, bounds.width > 0, bounds.height > 0, !laidOut else { return }
        laidOut = true
        diagnostics.record(.rootLaidOut)
    }

    @objc private func displayRefreshed() {
        diagnostics.record(.displayRefresh)
        displayLink?.invalidate()
        displayLink = nil
    }
}
