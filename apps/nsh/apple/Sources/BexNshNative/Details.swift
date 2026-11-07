import Foundation
import SwiftUI
#if os(macOS)
    import AppKit
#else
    import UIKit
#endif

/// These are Core's projected display values; no diff parsing or agent decisions live here.
struct DiffRow: Decodable {
    let text: String
    let old: UInt64?
    let new: UInt64?
    let kind: String
}

struct DiffFile: Decodable {
    let path: String
    let additions: UInt64?
    let deletions: UInt64?
    let rows: [DiffRow]
}

struct Agent: Decodable {
    let key: String
    let title: String
    let status: String
    let active: Bool
    let model: String?
    let detail: String?
    let depth: UInt32
}

enum DetailsContent: Decodable {
    case loading
    case sideChatLoading
    case diff(files: [DiffFile])
    case agents(rows: [Agent])
    case error(message: String)
}

private struct DetailsView: View {
    let content: DetailsContent

    var body: some View {
        Group {
            switch content {
            case .loading:
                ProgressView().accessibilityIdentifier("files.diff.loading")
            case .sideChatLoading:
                ProgressView("サイドチャットを準備中…").accessibilityIdentifier("sideChat.loading")
            case let .diff(files):
                if files.isEmpty {
                    ContentUnavailableView("変更はありません", systemImage: "checkmark.circle")
                } else {
                    ScrollView {
                        LazyVStack(spacing: 16) {
                            ForEach(files, id: \.path) { file in DiffCard(file: file) }
                        }.padding(12)
                    }
                }
            case let .agents(rows):
                if rows.isEmpty {
                    ContentUnavailableView("サブエージェントが起動するとここに表示されます。", systemImage: "network")
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 12) {
                            ForEach(rows, id: \.key) { agent in
                                VStack(alignment: .leading, spacing: 4) {
                                    HStack {
                                        if agent.active {
                                            ProgressView().controlSize(.small)
                                        }
                                        Text(verbatim: agent.title)
                                        Spacer()
                                        Text(verbatim: agent.status).foregroundStyle(.secondary)
                                    }
                                    if let detail = agent.detail {
                                        Text(verbatim: detail).foregroundStyle(.secondary)
                                    }
                                    if let model = agent
                                        .model {
                                        Text(verbatim: model).font(.caption).foregroundStyle(.secondary)
                                    }
                                }
                                .padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                                .padding(.leading, CGFloat(agent.depth) * 16)
                            }
                        }.font(.subheadline).padding(16).frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            case let .error(message):
                ContentUnavailableView("表示できませんでした", systemImage: "exclamationmark.triangle",
                                       description: Text(verbatim: message))
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct DiffCard: View {
    let file: DiffFile
    @State private var expanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(spacing: 0) {
                if file.rows.isEmpty {
                    Text("テキスト差分はありません").font(.caption).foregroundStyle(.secondary).padding()
                }
                ForEach(file.rows.indices, id: \.self) { index in
                    let row = file.rows[index]
                    HStack(alignment: .top, spacing: 0) {
                        if ["+", "-", " "].contains(row.kind) {
                            Text((row.new ?? row.old).map(String.init) ?? "")
                                .foregroundStyle(.secondary).frame(width: 40, alignment: .trailing).padding(
                                    .trailing,
                                    8
                                )
                        }
                        Text(verbatim: row.text.isEmpty ? " " : row.text)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                    }
                    .font(.system(.footnote, design: .monospaced))
                    .padding(.vertical, 3).padding(.horizontal, 8)
                    .background(row.kind == "+" ? Color.green.opacity(0.18) :
                        row.kind == "-" ? Color.red.opacity(0.18) :
                        row.kind == "@" ? Color.secondary.opacity(0.12) : Color.clear)
                }
            }
            #if os(iOS)
            .background(Color(uiColor: .systemBackground))
            #endif
        } label: {
            HStack(alignment: .top, spacing: 8) {
                Text(verbatim: file.path).fontWeight(.semibold).frame(maxWidth: .infinity, alignment: .leading)
                if let additions = file.additions {
                    Text("+\(additions)").foregroundStyle(.green)
                }
                if let deletions = file.deletions {
                    Text("−\(deletions)").foregroundStyle(.red)
                }
            }
            .font(.subheadline).foregroundStyle(.primary)
        }
        .accessibilityIdentifier("diff.file.\(file.path)")
        .padding(12)
        #if os(iOS)
            .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12))
        #else
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
        #endif
    }
}

@MainActor private final class DetailsOwner {
    #if os(macOS)
        let view = NSHostingView(rootView: DetailsView(content: .diff(files: [])))
    #else
        let view = NativeHostingView(rootView: DetailsView(content: .diff(files: [])))
    #endif

    func update(_ data: Data) {
        let content: DetailsContent
        do { content = try JSONDecoder().decode(DetailsContent.self, from: data) }
        catch { content = .error(message: error.localizedDescription) }
        #if os(macOS)
            view.rootView = DetailsView(content: content)
        #else
            view.controller.rootView = DetailsView(content: content)
        #endif
    }
}

@_cdecl("bex_nsh_details_create")
@MainActor public func detailsCreate(
    _: UnsafeMutableRawPointer?,
    _: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void
) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(DetailsOwner()).toOpaque()
}

@_cdecl("bex_nsh_details_view")
@MainActor public func detailsView(_ handle: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    Unmanaged.passUnretained(Unmanaged<DetailsOwner>.fromOpaque(handle).takeUnretainedValue().view).toOpaque()
}

@_cdecl("bex_nsh_details_update")
@MainActor public func detailsUpdate(_ handle: UnsafeMutableRawPointer, _ bytes: UnsafePointer<UInt8>, _ count: Int) {
    Unmanaged<DetailsOwner>.fromOpaque(handle).takeUnretainedValue().update(Data(bytes: bytes, count: count))
}

@_cdecl("bex_nsh_details_release")
@MainActor public func detailsRelease(_ handle: UnsafeMutableRawPointer) {
    let owner = Unmanaged<DetailsOwner>.fromOpaque(handle).takeRetainedValue()
    #if os(iOS)
        owner.view.detach()
    #else
        _ = owner
    #endif
}
