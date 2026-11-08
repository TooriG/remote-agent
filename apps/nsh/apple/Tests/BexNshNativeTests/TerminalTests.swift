#if os(macOS)
    import AppKit
    @testable import BexNshNative
    import XCTest

    @MainActor final class TerminalTests: XCTestCase {
        /// The Mac screen's text is read through SwiftTerm's internal terminal,
        /// which an upgrade could rename or replace.
        func testMacTerminalReadsItsVisibleRowsAsAccessibilityText() throws {
            let handle = terminalCreate(nil) { _, _, _, _, _, _ in }
            defer { terminalRelease(handle) }
            let view = Unmanaged<NSView>.fromOpaque(terminalView(handle)).takeUnretainedValue()
            view.frame = NSRect(x: 0, y: 0, width: 640, height: 240)
            let bytes = Array("first 日本語\r\nsecond\r\n".utf8)
            bytes.withUnsafeBufferPointer { terminalFeed(handle, $0.baseAddress, $0.count, 40, 6) }
            XCTAssertTrue(view.isAccessibilityElement())
            XCTAssertEqual(view.accessibilityRole(), .textArea)
            let deadline = Date().addingTimeInterval(5)
            while view.accessibilityValue() as? String != "first 日本語\nsecond", Date() < deadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            }
            XCTAssertEqual(view.accessibilityValue() as? String, "first 日本語\nsecond")
        }
    }
#endif
