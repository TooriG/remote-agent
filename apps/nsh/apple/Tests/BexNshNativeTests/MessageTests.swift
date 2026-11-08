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

        @MainActor func testFileChipsShowTheirGlyphAccentTooltipAndMenuAndReportTheirLink() throws {
            let json = """
            {"text": "main.rs · L4", "strong": false, "emphasis": false, "strikethrough": false, "code": true,
             "link": "src/main.rs:4", "darkColor": null, "lightColor": null,
             "file": {"icon": null, "folder": true, "tooltip": "/repo/src/main.rs:4 · 2 KB",
                      "menu": [{"id": "copy", "title": "フルパスをコピー"},
                               {"title": "エディターで開く", "children": [{"id": "code", "title": "VS Code"}]}],
                      "darkColor": 16711680, "lightColor": 255}}
            """
            let run = try JSONDecoder().decode(MessageRun.self, from: Data(json.utf8))
            let paragraph = MessageParagraph(header: nil, marker: nil, code: false, quoted: false, depth: 0,
                                             alignment: "left", runs: [run])
            let text = MessageAttributes.text([paragraph], secondary: false, dark: true)
            XCTAssertTrue(text.string.hasSuffix(" main.rs · L4"))
            XCTAssertNotNil(text.attribute(.attachment, at: 0, effectiveRange: nil), "the glyph leads the chip")
            let label = (text.string as NSString).range(of: "main.rs").location
            let font = try XCTUnwrap(text.attribute(.font, at: label, effectiveRange: nil) as? NSFont)
            XCTAssertFalse(font.isFixedPitch, "a chip drops its code span's font")
            XCTAssertEqual(text.attribute(.toolTip, at: label, effectiveRange: nil) as? String,
                           "/repo/src/main.rs:4 · 2 KB")
            let color = try XCTUnwrap(text.attribute(.foregroundColor, at: label, effectiveRange: nil) as? NSColor)
            XCTAssertEqual(color.redComponent, 1)
            XCTAssertNil(text.attribute(.underlineStyle, at: label, effectiveRange: nil))

            let view = MessageTextContainer()
            var actions: [[String: Any]] = []
            view.text.action = { actions.append($0) }
            view.update(MessageDocument(blocks: [paragraph], secondary: false, role: "assistant", target: "t",
                                        sideChat: false))
            _ = view.text.textView(view.text, clickedOnLink: "src/main.rs:4", at: label)
            XCTAssertEqual(actions.last?["text"] as? String, "src/main.rs:4")
            XCTAssertEqual(actions.last?["command"] as? Bool, false)
            let menu = try view.text.fileMenu(XCTUnwrap(run.file).menu, link: "src/main.rs:4")
            XCTAssertEqual(menu.items.map(\.title), ["フルパスをコピー", "エディターで開く"])
            XCTAssertEqual(menu.items[1].submenu?.items.first?.title, "VS Code")
            let item = try XCTUnwrap(menu.items[1].submenu?.items.first)
            _ = item.target?.perform(item.action, with: item)
            XCTAssertEqual(actions.last?["action"] as? String, "file")
            XCTAssertEqual(actions.last?["id"] as? String, "code")
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
