import Foundation
#if os(macOS)
    import AppKit

    typealias MessageFont = NSFont
    typealias MessageColor = NSColor
#else
    import UIKit

    typealias MessageFont = UIFont
    typealias MessageColor = UIColor
#endif

struct MessageRun: Decodable, Equatable {
    let text: String
    let strong: Bool
    let emphasis: Bool
    let strikethrough: Bool
    let code: Bool
    let link: String?
    let darkColor: UInt32?
    let lightColor: UInt32?
}

struct MessageParagraph: Decodable, Equatable {
    let header: Int?
    let marker: String?
    let code: Bool
    let quoted: Bool
    let depth: UInt32
    let alignment: String
    let runs: [MessageRun]
}

struct MessageDocument: Decodable, Equatable {
    let blocks: [MessageParagraph]
    let secondary: Bool
    /// Whose text it is, `user` or `assistant`, as its accessibility identifier names it.
    let role: String
    let target: String
    let sideChat: Bool

    static let empty = MessageDocument(blocks: [], secondary: false, role: "assistant", target: "", sideChat: false)
}

@MainActor enum MessageAttributes {
    static func text(_ blocks: [MessageParagraph], secondary: Bool, dark: Bool) -> NSAttributedString {
        let result = NSMutableAttributedString(string: "")
        for (index, block) in blocks.enumerated() {
            let start = result.length
            #if os(macOS)
                let base: CGFloat = 14
            #else
                let base: CGFloat = block.code ? 15 : 18
            #endif
            let size = block.header.map { [26.0, 23, 21, 19, 18, 18][min(5, max(0, $0 - 1))] } ?? base
            if let marker = block.marker {
                result.append(NSAttributedString(string: marker + " ", attributes: [
                    .font: font(size: size, mono: block.code, bold: false, italic: false),
                    .foregroundColor: color(secondary: secondary)
                ]))
            }
            for run in block.runs {
                result.append(NSAttributedString(string: run.text, attributes: attributes(
                    run, size: size, code: block.code, heading: block.header != nil,
                    secondary: secondary, dark: dark
                )))
            }
            let paragraph = NSMutableParagraphStyle()
            paragraph.alignment = block.alignment == "center" ? .center : block.alignment == "right" ? .right : .left
            paragraph.lineSpacing = block.code ? 0 : 5
            paragraph.paragraphSpacing = index + 1 == blocks.count ? 0 : 14
            let indent = CGFloat(max(0, Int(block.depth) - 1)) * 22 + (block.quoted ? 12 : 0)
            paragraph.firstLineHeadIndent = indent
            paragraph.headIndent = indent + (block.depth > 0 ? 22 : 0)
            if index + 1 < blocks.count {
                result.append(NSAttributedString(string: "\n", attributes: [
                    .font: font(size: size, mono: block.code, bold: false, italic: false)
                ]))
            }
            result.addAttribute(.paragraphStyle, value: paragraph,
                                range: NSRange(location: start, length: result.length - start))
        }
        return result
    }

    private static func color(secondary: Bool) -> MessageColor {
        #if os(macOS)
            secondary ? .secondaryLabelColor : .labelColor
        #else
            secondary ? .secondaryLabel : .label
        #endif
    }

    private static func attributes(_ run: MessageRun, size: CGFloat, code: Bool, heading: Bool,
                                   secondary: Bool, dark: Bool) -> [NSAttributedString.Key: Any] {
        var result: [NSAttributedString.Key: Any] = [
            .font: font(size: size, mono: code || run.code, bold: heading || run.strong, italic: run.emphasis),
            .foregroundColor: color(secondary: secondary)
        ]
        if let rgb = dark ? run.darkColor : run.lightColor {
            result[.foregroundColor] = MessageColor(red: CGFloat((rgb >> 16) & 255) / 255,
                                                    green: CGFloat((rgb >> 8) & 255) / 255,
                                                    blue: CGFloat(rgb & 255) / 255, alpha: 1)
        }
        if let url = run.link.flatMap(URL.init(string:)) {
            result[.link] = url
        }
        if run.strikethrough {
            result[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        if run.code {
            #if os(macOS)
                result[.backgroundColor] = NSColor.quaternaryLabelColor
            #else
                result[.backgroundColor] = UIColor.secondarySystemBackground
            #endif
        }
        return result
    }

    private static func font(size: CGFloat, mono: Bool, bold: Bool, italic: Bool) -> MessageFont {
        let font = mono ? MessageFont.monospacedSystemFont(ofSize: size, weight: bold ? .semibold : .regular)
            : MessageFont.systemFont(ofSize: size, weight: bold ? .semibold : .regular)
        #if os(macOS)
            return italic ? NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask) : font
        #else
            let base = italic ? font.fontDescriptor.withSymbolicTraits(.traitItalic)
                .map { UIFont(descriptor: $0, size: size) } ?? font : font
            return UIFontMetrics.default.scaledFont(for: base)
        #endif
    }
}
