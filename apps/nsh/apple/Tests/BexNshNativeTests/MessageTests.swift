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
                                        sideChat: document.sideChat, maxWidth: 70))
            view.layout()
            XCTAssertGreaterThan(try XCTUnwrap(heights.last), wide)
        }

        @MainActor func testRecycledTextReportsOnlyTheHeightAtItsNewColumnsWidth() throws {
            let run = MessageRun(text: String(repeating: "A long response retains its reading position. ", count: 80),
                                 strong: false, emphasis: false, strikethrough: false, code: false,
                                 link: nil, darkColor: nil, lightColor: nil)
            let block = MessageParagraph(header: nil, marker: nil, code: false, quoted: false,
                                         depth: 0, alignment: "left", runs: [run])
            let document = MessageDocument(blocks: [block], secondary: false, role: "assistant",
                                           target: "fixture", sideChat: false, maxWidth: 740)
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
            XCTAssertEqual(font.pointSize, 23)
            XCTAssertTrue(NSFontManager.shared.traits(of: font).contains(.boldFontMask))
        #endif
    }
}
