import Foundation
import UIKit
import XCTest

/// Terminal selection is rendered and handled by SwiftTerm rather than UITextInput.
extension LaunchUITests {
    func testSimulatorNativeTerminalCopiesTextOnLongPress() throws {
        let app = try connectedSimulatorApp()
        app.buttons["tasks.new.project.simulator-project"].tap()
        app.buttons["task.tools"].tap()
        openWorkbenchTool(app, "terminal")
        assertTerminalReady(app)
        let terminal = app.descendants(matching: .any)["terminal.screen"]
        terminal.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        terminal.typeText("printf '\\033[2J\\033[HNeedle Alpha Bravo\\n'; exit 0\n")
        let status = app.staticTexts["終了 · 0"]
        XCTAssertTrue(status.waitForExistence(timeout: 10))
        waitForStableFrame(terminal)
        // Anchor below the status text, which shifts the terminal when the shell exits.
        let word = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: terminal.frame.minX + 20 - app.frame.minX,
            dy: status.frame.maxY + 8 - app.frame.minY
        ))
        word.press(forDuration: 1.2)
        let copy = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Copy", "コピー"])).firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 5))
        captureScreen(app, named: "Terminal long press selects a word and exposes Copy")
        // A second long press on Bravo reopens the menu without changing Needle's selection.
        word.withOffset(CGVector(dx: 100, dy: 0)).press(forDuration: 1.2)
        XCTAssertTrue(copy.waitForExistence(timeout: 5)); copy.tap()
        assertTerminalCopyInDraft(app, text: "Needle")
        captureScreen(app, named: "Terminal long press preserves and copies only the selected word")
    }

    func testSimulatorNativeTerminalAdjustsSelectionHandles() throws {
        let app = try connectedSimulatorApp()
        app.buttons["tasks.new.project.simulator-project"].tap()
        app.buttons["task.tools"].tap()
        openWorkbenchTool(app, "terminal")
        assertTerminalReady(app)
        let terminal = app.descendants(matching: .any)["terminal.screen"]
        terminal.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        // Leave room above the selection for its menu and start handle.
        terminal.typeText("printf '\\033[2J\\033[6;1HPrefix Needle Alpha Bravo\\nCharlie Delta\\n'; exit 0\n")
        let status = app.staticTexts["終了 · 0"]
        XCTAssertTrue(status.waitForExistence(timeout: 10))
        waitForStableFrame(terminal)
        let font = UIFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        let cellWidth = "W".size(withAttributes: [.font: font]).width
        let cellHeight = ceil(font.lineHeight)
        let origin = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: terminal.frame.minX - app.frame.minX, dy: status.frame.maxY - app.frame.minY
        ))
        origin.withOffset(CGVector(dx: 9 * cellWidth, dy: 5.5 * cellHeight)).press(forDuration: 1.2)
        let copy = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Copy", "コピー"])).firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 5))
        // Extend the end handle onto the next line, then move the start to Prefix.
        let end = origin.withOffset(CGVector(dx: 13 * cellWidth, dy: 6 * cellHeight + 6))
        end.press(forDuration: 0.1, thenDragTo: origin.withOffset(CGVector(dx: 7.5 * cellWidth, dy: 6.5 * cellHeight)),
                  withVelocity: .slow, thenHoldForDuration: 0)
        XCTAssertTrue(copy.waitForExistence(timeout: 5))
        let start = origin.withOffset(CGVector(dx: 7 * cellWidth, dy: 5 * cellHeight - 6))
        start.press(
            forDuration: 0.1,
            thenDragTo: origin.withOffset(CGVector(dx: 0.5 * cellWidth, dy: 5.5 * cellHeight)),
            withVelocity: .slow,
            thenHoldForDuration: 0
        )
        XCTAssertTrue(copy.waitForExistence(timeout: 5))
        captureScreen(app, named: "Terminal selection handles adjust both ends across lines")
        copy.tap()
        assertTerminalCopyInDraft(app, text: "Prefix Needle Alpha Bravo\nCharlie")
    }

    private func assertTerminalCopyInDraft(_ app: XCUIApplication, text: String) {
        closeWorkbench(app)
        let composer = app.textFields["task.message"]
        // Project drafts survive earlier tests and app relaunches.
        replaceFieldText(composer, text: "")
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        composer.press(forDuration: 1.2)
        let paste = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "ペースト"])).firstMatch
        XCTAssertTrue(paste.waitForExistence(timeout: 5)); paste.tap()
        waitForDraftText(composer, text: text)
    }
}
