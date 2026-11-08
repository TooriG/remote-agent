@testable import BexNshNative
import XCTest
#if os(macOS)
    import AppKit
#endif

final class MessageTests: XCTestCase {
    #if os(macOS)
        @MainActor func testTextKitOwnsTheVisibleTextAndReflowsAtTheNativeWidth() throws {
            let url = try XCTUnwrap(Bundle.module.url(forResource: "message", withExtension: "json"))
            let document = try JSONDecoder().decode(MessageDocument.self, from: Data(contentsOf: url))
            let view = MessageTextContainer()
            var heights: [CGFloat] = []
            view.send = {
                if let height = $0["measuredHeight"] as? CGFloat {
                    heights.append(height)
                }
            }
            view.update(document)
            XCTAssertEqual(view.text.string, "日本語 👋 ファイル")
            XCTAssertEqual(view.text.accessibilityIdentifier(), "message.user-text")
            XCTAssertNotNil(view.text.textStorage)
            XCTAssertNotNil(view.text.layoutManager)
            view.frame = CGRect(x: 0, y: 0, width: 400, height: 100)
            view.layout()
            let wide = try XCTUnwrap(heights.last)
            view.frame.size.width = 70
            view.layout()
            XCTAssertGreaterThan(try XCTUnwrap(heights.last), wide)
        }
    #endif

    @MainActor func testNativeTextPreservesTheSharedContentAndOpaqueActionTarget() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "message", withExtension: "json"))
        let document = try JSONDecoder().decode(MessageDocument.self, from: Data(contentsOf: url))
        let attributed = MessageAttributes.text(document.blocks, secondary: document.secondary, dark: true)
        XCTAssertEqual(attributed.string, "日本語 👋 ファイル")
        XCTAssertEqual(document.target, "opaque-native-menu-target")
        XCTAssertTrue(document.sideChat)
        XCTAssertEqual(document.blocks[0].header, 2)
        let position = (attributed.string as NSString).range(of: "ファイル").location
        let link = attributed.attribute(.link, at: position, effectiveRange: nil) as? URL
        XCTAssertEqual(link?.absoluteString, "file:///fixture/%E6%97%A5%E6%9C%AC%E8%AA%9E.txt")
        #if os(macOS)
            let font = try XCTUnwrap(attributed.attribute(.font, at: 0, effectiveRange: nil) as? NSFont)
            XCTAssertEqual(font.pointSize, 23)
            XCTAssertTrue(NSFontManager.shared.traits(of: font).contains(.boldFontMask))
        #endif
    }
}
