import Foundation
import UIKit

@main
struct AppDiagnosticsTests {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("FAIL: \(message)\n", stderr)
            exit(1)
        }
    }

    static func records(_ file: URL) throws -> [[String: Any]] {
        try String(contentsOf: file, encoding: .utf8).split(separator: "\n").compactMap {
            try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any]
        }
    }

    static func checkResponsiveness() {
        var probe = MainThreadProbe()
        check(probe.poll(at: 10) == .ping, "Probe the main queue once")
        check(probe.poll(at: 11.999) == nil, "Short scheduling delays are not hangs")
        check(probe.poll(at: 12) == .stalled(2000), "Record a two-second main-queue delay")
        check(probe.poll(at: 30) == nil, "An unresponsive queue must not flood the journal")
        check(probe.reply(sentAt: 10, at: 31) == 21000, "Record recovery with the whole delay")
        check(probe.poll(at: 32) == .ping, "Keep monitoring after recovery")
        check(probe.reply(sentAt: 32, at: 32.01) == nil, "Responsive probes must not write logs")
        check(probe.poll(at: 33) == .ping, "Begin another probe")
        check(probe.reply(sentAt: 33, at: 35.5) == 2500,
              "A late reply must retain its delay even if the sampler has not reported it yet")

        check(probe.poll(at: 40) == .ping, "Begin a probe before backgrounding")
        probe.setActive(false)
        check(probe.poll(at: 900) == nil, "Do not probe the main queue while inactive/backgrounded")
        probe.setActive(true)
        check(probe.poll(at: 1000) == .ping, "Suspension must not count as an unresponsive main queue")
        check(probe.reply(sentAt: 40, at: 1000.1) == nil, "Ignore a callback queued before suspension")
        check(probe.poll(at: 1002) == .stalled(2000), "An old callback must not clear a new foreground probe")
    }

    static func checkJournal(_ directory: URL) async throws {
        let file = directory.appendingPathComponent("app.jsonl")
        let first = AppDiagnostics(directory: directory)
        first.record(.launchStarted)
        first.record(.profilesLoaded, value: 2)
        await first.flush()
        let before = try records(file)
        check(before.compactMap { $0["event"] as? String } == ["launchStarted", "profilesLoaded"],
              "Persist startup milestones without a Host, Store, or main run loop")
        check(before.last?["value"] as? Int == 2, "Retain numeric measurements")
        check(
            Set(before[0].keys) == Set([
                "timestampMs",
                "uptimeMs",
                "launch",
                "pid",
                "build",
                "revision",
                "event",
                "value"
            ]),
            "The log schema must contain only diagnostic metadata, fixed events, and a number"
        )
        check(before[0]["launch"] as? String == before[1]["launch"] as? String, "Correlate one process's events")
        let second = AppDiagnostics(directory: directory)
        second.record(.launchStarted)
        await second.flush()
        let after = try records(file)
        check(after.count == 3, "A relaunch must preserve the preceding launch's evidence")
        check(after[0]["launch"] as? String != after[2]["launch"] as? String, "Distinguish successive launches")
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        check(mode == 0o600, "Diagnostic files must be private")

        let output = try FileHandle(forWritingTo: file)
        try output.seekToEnd()
        try output.write(contentsOf: Data("{\"interrupted\":".utf8))
        try output.close()
        second.record(.modelReady)
        await second.flush()
        let resumed = try records(file)
        check(resumed.count == 4 && resumed.last?["event"] as? String == "modelReady",
              "Logging must recover from a truncated write within the same launch")
        var interrupted = try Data(contentsOf: file)
        interrupted.append(contentsOf: "{\"interrupted\":".utf8)
        try interrupted.write(to: file)
        let recovered = AppDiagnostics(directory: directory)
        recovered.record(.launchStarted)
        await recovered.flush()
        let restored = try records(file)
        check(restored.count == 5 && restored.last?["event"] as? String == "launchStarted",
              "An interrupted final write must not swallow the next launch marker")
    }

    static func checkRotation(_ directory: URL) async throws {
        let journal = AppDiagnostics(directory: directory, fileBytes: 2048)
        for value in 0 ..< 60 {
            journal.record(.mainThreadRecovered, value: value)
        }
        await journal.flush()
        let files = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        check(Set(files.map(\.lastPathComponent)) == Set(["app.jsonl", "app.jsonl.1", "app.jsonl.2"]),
              "Retain only the active file and two bounded archives")
        for file in files {
            let bytes = try Data(contentsOf: file)
            let entries = try records(file)
            check(bytes.count <= 2048, "Each archive must stay within the size limit")
            check(!entries.isEmpty && entries.count == bytes.split(separator: 0x0A).count,
                  "Rotation must preserve whole JSON records")
        }
        let current = try records(directory.appendingPathComponent("app.jsonl"))
        check(current.last?["value"] as? Int == 59, "Rotation must retain the newest event")
    }

    static func checkWriteRecovery(_ directory: URL) async throws {
        try Data().write(to: directory) // A file blocks creation of the log directory.
        let journal = AppDiagnostics(directory: directory)
        journal.record(.launchStarted)
        await journal.flush()
        try FileManager.default.removeItem(at: directory)
        journal.record(.modelReady)
        await journal.flush()
        let entries = try records(directory.appendingPathComponent("app.jsonl"))
        check(entries.count == 1 && entries[0]["event"] as? String == "modelReady",
              "A logging error must neither crash the app nor disable later logging")
    }

    @MainActor static func checkView(_ directory: URL) async throws {
        let journal = AppDiagnostics(directory: directory)
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 640))
        let view = AppDiagnosticUIView(diagnostics: journal)
        check(!view.isOpaque && !view.isUserInteractionEnabled && view.backgroundColor == .clear,
              "The observer must not cover content or intercept input")
        window.addSubview(view)
        view.layoutIfNeeded()
        view.frame = window.bounds
        view.setNeedsLayout()
        view.layoutIfNeeded()
        view.setNeedsLayout()
        view.layoutIfNeeded()
        view.removeFromSuperview()
        await journal.flush()
        let entries = try records(directory.appendingPathComponent("app.jsonl"))
        check(entries.compactMap { $0["event"] as? String } == ["windowAttached", "rootLaidOut", "windowDetached"],
              "Record attachment and the first nonempty layout, without flooding on later layouts")
    }

    @MainActor static func main() async throws {
        checkResponsiveness()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try await checkJournal(directory.appendingPathComponent("journal"))
        try await checkRotation(directory.appendingPathComponent("rotation"))
        try await checkWriteRecovery(directory.appendingPathComponent("blocked"))
        try await checkView(directory.appendingPathComponent("view"))
        print("App diagnostics tests passed")
    }
}
