import NativeBrowserViews
import SwiftUI

#if os(macOS)
    import AppKit

    private typealias BrowserImage = NSImage
#else
    import UIKit

    private typealias BrowserImage = UIImage
#endif

private typealias BrowserCallback = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void

struct BrowserData: Decodable {
    let image: String
    let enabled: Bool
    let dialog: BrowserDialog?
    let credentials: NativeBrowserCredentials?

    init(data: Data) throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        self = try decoder.decode(Self.self, from: data)
    }
}

struct BrowserDialog: Decodable { let message: String; let prompt: Bool }
@MainActor private final class BrowserOwner {
    #if os(macOS)
        let view = NSHostingView(rootView: BrowserViewport(
            image: nil,
            enabled: false,
            dialog: nil,
            credentials: nil,
            send: { _ in }
        ))
    #else
        let view = NativeHostingView(rootView: BrowserViewport(
            image: nil,
            enabled: false,
            dialog: nil,
            credentials: nil,
            send: { _ in }
        ))
    #endif
    let context: UnsafeMutableRawPointer?
    var callback: BrowserCallback?
    private var encodedImage = ""
    private var image: BrowserImage?

    init(context: UnsafeMutableRawPointer?, callback: @escaping BrowserCallback) {
        self.context = context
        self.callback = callback
    }

    func update(_ data: Data) {
        guard let content = try? BrowserData(data: data) else { return }
        if content.image != encodedImage {
            encodedImage = content.image
            image = Data(base64Encoded: content.image).flatMap(BrowserImage.init(data:))
        }
        let root = BrowserViewport(
            image: image,
            enabled: content.enabled,
            dialog: content.dialog,
            credentials: content.credentials
        ) { [weak self] value in
            guard let self, let data = try? JSONSerialization.data(withJSONObject: value) else { return }
            data.withUnsafeBytes { self.callback?(self.context, $0.bindMemory(to: UInt8.self).baseAddress, data.count) }
        }
        #if os(macOS)
            view.rootView = root
        #else
            view.controller.rootView = root
        #endif
    }
}

private struct BrowserViewport: View {
    let image: BrowserImage?
    let enabled: Bool
    let dialog: BrowserDialog?
    let credentials: NativeBrowserCredentials?
    let send: ([String: Any]) -> Void
    @State private var prompt = ""

    var body: some View {
        Group {
            if let image {
                RemoteBrowserCanvas(image: image, acceptsInput: enabled) { action in
                    switch action {
                    case let .click(x, y): send(["Click": ["x": x, "y": y]])
                    case let .scroll(x, y, dx, dy): send(["Scroll": ["x": x, "y": y, "delta_x": dx, "delta_y": dy]])
                    }
                }
            } else {
                ContentUnavailableView("プレビューはまだありません", systemImage: "globe",
                                       description: Text("URLを入力するか、AIにブラウザ操作を依頼してください。"))
            }
        }.accessibilityIdentifier("browser.content")
            .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
                send(["ViewportSize": ["width": size.width, "height": size.height]])
            }
            .alert(Text(verbatim: dialog?.message ?? ""), isPresented: Binding(get: { dialog != nil }, set: { _ in })) {
                if dialog?.prompt == true {
                    TextField("入力", text: $prompt)
                }
                Button("キャンセル", role: .cancel) { send(["Dialog": ["accept": false, "text": ""]]); prompt = "" }
                Button("OK") { send(["Dialog": ["accept": true, "text": prompt]]); prompt = "" }
            }
            .sheet(isPresented: Binding(
                get: { credentials != nil },
                set: {
                    if !$0 {
                        send(["DismissCredentials": [:]])
                    }
                }
            )) {
                if let credentials {
                    credentialsView(credentials)
                }
            }
    }

    private func credentialsView(_ credentials: NativeBrowserCredentials) -> some View {
        BrowserCredentialsView(credentials: credentials, send: { action in
            switch action {
            case let .importCookies(profile): send(["ImportCookies": ["profile": profile]])
            case let .importPasswords(profile): send(["ImportPasswords": ["profile": profile]])
            case let .fillPassword(origin, username): send(["FillPassword": ["origin": origin, "username": username]])
            case let .deletePasswords(origin): send(["DeletePasswords": ["origin": origin]])
            }
        }, close: { send(["DismissCredentials": [:]]) })
    }
}

@_cdecl("bex_nsh_browser_create")
@MainActor public func browserCreate(
    _ context: UnsafeMutableRawPointer?,
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, Int) -> Void
) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(BrowserOwner(context: context, callback: callback)).toOpaque()
}

@_cdecl("bex_nsh_browser_view")
@MainActor public func browserView(_ handle: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    let owner = Unmanaged<BrowserOwner>.fromOpaque(handle).takeUnretainedValue()
    #if os(macOS)
        return Unmanaged.passUnretained(owner.view).toOpaque()
    #else
        return Unmanaged.passUnretained(owner.view).toOpaque()
    #endif
}

@_cdecl("bex_nsh_browser_update")
@MainActor public func browserUpdate(_ handle: UnsafeMutableRawPointer, _ bytes: UnsafePointer<UInt8>, _ count: Int) {
    Unmanaged<BrowserOwner>.fromOpaque(handle).takeUnretainedValue().update(Data(bytes: bytes, count: count))
}

@_cdecl("bex_nsh_browser_release")
@MainActor public func browserRelease(_ handle: UnsafeMutableRawPointer) {
    let owner = Unmanaged<BrowserOwner>.fromOpaque(handle).takeRetainedValue()
    owner.callback = nil
    #if os(iOS)
        owner.view.controller.dismiss(animated: false)
        owner.view.detach()
    #endif
}
