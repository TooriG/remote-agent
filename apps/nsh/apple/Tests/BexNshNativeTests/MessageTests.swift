@testable import BexNshNative
import XCTest
#if os(macOS)
    import AppKit
#endif

final class MessageTests: XCTestCase {
    #if os(macOS)
        @MainActor func testTextKitVisibilityFollowsLayerScrollingBeforeAppKitBoundsCommit() {
            let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 400, height: 200),
                                  styleMask: [], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            defer { window.close() }
            let clip = NSView(frame: CGRect(x: 0, y: 0, width: 400, height: 200))
            clip.wantsLayer = true
            clip.layer?.masksToBounds = true
            window.contentView = clip
            let content = NSView(frame: CGRect(x: 0, y: 0, width: 400, height: 1000))
            content.wantsLayer = true
            clip.addSubview(content)
            let text = MessageTextView()
            text.frame = CGRect(x: 0, y: 600, width: 400, height: 40)
            content.addSubview(text)
            window.orderFront(nil)
            window.displayIfNeeded()
            text.watchScrollingGeometry()
            text.layer?.displayIfNeeded()
            XCTAssertEqual(text.layer?.needsDisplay(), false)
            XCTAssertTrue(text.visibleRect.isEmpty)
            let committed = content.bounds
            content.layer?.sublayerTransform = CATransform3DMakeTranslation(0, -600, 0)
            XCTAssertEqual(content.bounds, committed)
            XCTAssertEqual(text.visibleRect.height, 40, "glyphs must draw before the deferred NSView bounds commit")
            XCTAssertEqual(text.layer?.needsDisplay(), true, "layer scrolling must invalidate newly visible native glyphs")
            text.layer?.displayIfNeeded()
            content.layer?.sublayerTransform = CATransform3DMakeTranslation(0, -590, 0)
            XCTAssertEqual(text.visibleRect.height, 40)
            XCTAssertEqual(text.layer?.needsDisplay(), false, "moving a fully painted visible cell needs no redraw")
            content.layer?.sublayerTransform = CATransform3DMakeTranslation(0, -200, 0)
            XCTAssertTrue(text.visibleRect.isEmpty)
            XCTAssertEqual(text.layer?.needsDisplay(), false, "an offscreen cell must not be invalidated")
        }

        @MainActor func testExposureKeepsOverlappingPixelsForBothScrollDirectionsAndResize() {
            let before = NSRect(x: 0, y: 0, width: 400, height: 200)
            let after = NSRect(x: 0, y: 60, width: 400, height: 200)
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: before, current: after),
                           [NSRect(x: 0, y: 200, width: 400, height: 60)])
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: after, current: before),
                           [NSRect(x: 0, y: 0, width: 400, height: 60)])
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: before, current: before), [])
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: before, current: .zero), [])
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: .zero, current: before), [before])
            let outside = NSRect(x: 0, y: 300, width: 400, height: 200)
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: before, current: outside), [outside])
            let inside = NSRect(x: 20, y: 20, width: 360, height: 160)
            XCTAssertEqual(MessageTextView.newlyExposedRects(previous: inside, current: before), [
                NSRect(x: 0, y: 0, width: 400, height: 20),
                NSRect(x: 0, y: 180, width: 400, height: 20),
                NSRect(x: 0, y: 20, width: 20, height: 160),
                NSRect(x: 380, y: 20, width: 20, height: 160),
            ])
        }

        #if DEBUG
            @MainActor func testStreamProbeRetainsFirstPaintAcrossAppendsAndClearsReplacedContent() {
                var paints = RenderProbe.Paints()
                paints.record("update 0", at: 10)
                paints.update(previous: "update 0", current: "update 0\nupdate 1")
                paints.record("update 0\nupdate 1", at: 20)
                XCTAssertEqual(paints.first(for: "update 0"), 10)
                XCTAssertEqual(paints.first(for: "update 1"), 20)
                // A later refresh of the same text must not replace its first draw.
                paints.update(previous: "update 0\nupdate 1", current: "update 0\nupdate 1")
                paints.record("update 0\nupdate 1", at: 30)
                XCTAssertEqual(paints.first(for: "update 1"), 20)
                paints.update(previous: "update 0\nupdate 1", current: "replacement")
                XCTAssertNil(paints.first(for: "update 0"))
                paints.record("replacement", at: 40)
                paints.clear()
                XCTAssertNil(paints.first(for: "replacement"))
            }
        #endif

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
            // NSH supplies the new width with the document. The old native
            // frame can belong to a different row while a cell is recycled.
            view.update(MessageDocument(blocks: document.blocks, secondary: document.secondary,
                                        role: document.role, target: document.target,
                                        sideChat: document.sideChat, maxWidth: 70, menu: document.menu))
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
                                        sideChat: false, maxWidth: 400,
                                        menu: MessageMenuTitles(quote: "Add to chat", sideChat: "Ask in side chat")))
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

        @MainActor func testALineIsT3CodesLineTallWhateverItsScripts() throws {
            // A bracket falls back to a font whose leading would grow its line.
            for (role, text) in [("assistant", "シミュレータで完了しました。"), ("user", "[groups] 完了した作業を確認します")] {
                let run = MessageRun(text: text, strong: false, emphasis: false, strikethrough: false, code: false,
                                     link: nil, darkColor: nil, lightColor: nil)
                let block = MessageParagraph(header: nil, marker: nil, code: false, quoted: false,
                                             depth: 0, alignment: "left", runs: [run])
                let view = MessageTextContainer()
                var heights: [CGFloat] = []
                view.send = { if let height = $0["measuredHeight"] as? CGFloat { heights.append(height) } }
                view.update(MessageDocument(blocks: [block], secondary: false, role: role, target: "t",
                                            sideChat: false, maxWidth: 600,
                                            menu: MessageMenuTitles(quote: "", sideChat: "")))
                // T3 Code's `text-sm leading-relaxed`: 22.75 points.
                XCTAssertEqual(heights.last, 23, role)
            }
        }

        @MainActor func testItemsOfOneListSitCloserThanTwoLists() throws {
            let item = { (marker: String, text: String) in
                MessageParagraph(header: nil, marker: marker, code: false, quoted: false, depth: 1,
                                 alignment: "left",
                                 runs: [MessageRun(text: text, strong: false, emphasis: false, strikethrough: false,
                                                   code: false, link: nil, darkColor: nil, lightColor: nil)])
            }
            let blocks = [item("1.", "first"), item("2.", "second"), item("•", "bullet")]
            let text = MessageAttributes.text(blocks, secondary: false, dark: true)
            let spacing = { (word: String) -> CGFloat in
                let at = (text.string as NSString).range(of: word).location
                let style = text.attribute(.paragraphStyle, at: at, effectiveRange: nil) as? NSParagraphStyle
                return style?.paragraphSpacing ?? -1
            }
            // T3 Code's `li` 0.25rem apart; its lists 0.65rem.
            XCTAssertEqual(spacing("first"), 4)
            XCTAssertEqual(spacing("second"), 10.4, accuracy: 0.01)
        }

        @MainActor func testTableCellsUseT3CodesSmallerTextTheirHeaderSemibold() throws {
            let run = MessageRun(text: "構成", strong: false, emphasis: false, strikethrough: false, code: false,
                                 link: nil, darkColor: nil, lightColor: nil)
            let block = MessageParagraph(header: nil, marker: nil, code: false, quoted: false,
                                         depth: 0, alignment: "left", runs: [run])
            for (cell, bold) in [("header", true), ("body", false)] {
                let text = MessageAttributes.text([block], secondary: false, dark: true, answer: true, cell: cell)
                let font = try XCTUnwrap(text.attribute(.font, at: 0, effectiveRange: nil) as? NSFont)
                // `.chat-markdown table`: 0.75rem, its `thead th` weight 600.
                XCTAssertEqual(font.pointSize, 12, cell)
                let traits = font.fontDescriptor.object(forKey: .traits) as? [NSFontDescriptor.TraitKey: Any]
                let weight = traits?[.weight] as? CGFloat ?? 0
                XCTAssertEqual(weight >= NSFont.Weight.semibold.rawValue, bold, cell)
                let view = MessageTextContainer()
                var heights: [CGFloat] = []
                view.send = { if let height = $0["measuredHeight"] as? CGFloat { heights.append(height) } }
                view.update(MessageDocument(blocks: [block], secondary: false, role: "assistant", target: "t",
                                            sideChat: false, maxWidth: 600,
                                            menu: MessageMenuTitles(quote: "", sideChat: ""), cell: cell))
                // 12 points at `leading-relaxed`: 19.5.
                XCTAssertEqual(heights.last, 20, cell)
            }
        }

        @MainActor func testRecycledTextReportsOnlyTheHeightAtItsNewColumnsWidth() throws {
            let run = MessageRun(text: String(repeating: "A long response retains its reading position. ", count: 80),
                                 strong: false, emphasis: false, strikethrough: false, code: false,
                                 link: nil, darkColor: nil, lightColor: nil)
            let block = MessageParagraph(header: nil, marker: nil, code: false, quoted: false,
                                         depth: 0, alignment: "left", runs: [run])
            let document = MessageDocument(blocks: [block], secondary: false, role: "assistant",
                                           target: "fixture", sideChat: false, maxWidth: 740,
                                           menu: MessageMenuTitles(quote: "", sideChat: ""))
            let view = MessageTextContainer()
            // A short user bubble occupied the same recycled native view.
            view.frame = CGRect(x: 0, y: 0, width: 167, height: 17)
            var heights: [CGFloat] = []
            view.send = { if let height = $0["measuredHeight"] as? CGFloat { heights.append(height) } }
            view.update(document)
            let first = try XCTUnwrap(heights.first)
            view.layout()
            view.frame.size.width = 740
            view.layout()
            XCTAssertEqual(heights, [first], "a recycled frame must not publish a second, incorrect row height")
            XCTAssertEqual(view.text.textContainer?.containerSize.width, 740)
            XCTAssertGreaterThan(first, 17)
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
            // T3 Code's `h2`: 1.125rem.
            XCTAssertEqual(font.pointSize, 18)
            XCTAssertTrue(NSFontManager.shared.traits(of: font).contains(.boldFontMask))
        #endif
    }
}
