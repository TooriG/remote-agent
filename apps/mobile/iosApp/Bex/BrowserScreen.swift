import AgentCore
import SwiftUI
import UIKit

struct WorkspaceBrowserScreen: View {
    let store: AgentStore
    let thread: SessionRef
    @Environment(\.scenePhase) private var scenePhase
    @State private var frame: BrowserFrame?
    @State private var image: UIImage?
    @State private var address = ""
    @State private var text = ""
    @State private var credentials: BrowserCredentials?
    @State private var notice: String?
    @State private var error: String?
    @State private var connectionError: String?
    @State private var commands: [BrowserRequest] = []
    @FocusState private var addressFocused: Bool
    @State private var showKeyboard = false
    @State private var dialogText = ""
    @State private var viewportSize = CGSize.zero

    private var acceptsInput: Bool {
        frame != nil && scenePhase == .active && connectionError == nil
    }

    private var currentTab: BrowserTab? {
        frame?.tabs.first { $0.id == frame?.tabId }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                TextField("検索または URL", text: $address)
                    .keyboardType(.webSearch).textInputAutocapitalization(.never).disableAutocorrection(true)
                    .textFieldStyle(.roundedBorder).focused($addressFocused).submitLabel(.go).onSubmit(open)
                    .accessibilityIdentifier("browser.address")
                Button { send(.reload) } label: {
                    Image(systemName: "arrow.clockwise").frame(width: 44, height: 44)
                }
                .accessibilityLabel("再読み込み")
            }.font(.body).padding(.horizontal, 8).padding(.vertical, 4).disabled(!acceptsInput)
            Divider().opacity(0.6)
            if let message = error ?? connectionError ?? notice {
                AppNotice(text: message).padding(.horizontal)
            }
            if let dialog = frame?.dialog {
                VStack {
                    Text(dialog.message)
                    if dialog.prompt {
                        TextField("入力", text: $dialogText).textFieldStyle(.roundedBorder)
                    }
                    HStack {
                        Button("キャンセル") { send(.dialog(accept: false, text: "")) }
                        Button("OK") { send(.dialog(accept: true, text: dialogText)); dialogText = "" }
                    }.disabled(!acceptsInput)
                }.padding()
            }
            ZStack {
                if let image, currentTab?.url != "about:blank" {
                    RemoteBrowserCanvas(image: image, acceptsInput: acceptsInput, send: send)
                        .accessibilityIdentifier("browser.content")
                } else if frame != nil {
                    ContentUnavailableView("プレビューはまだありません", systemImage: "globe",
                                           description: Text("URLを入力するか、AIにブラウザ操作を依頼してください。"))
                } else {
                    ProgressView("Host のブラウザに接続中…")
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(GeometryReader { geometry in
                    Color.clear.onAppear { viewportSize = geometry.size }
                        .onChange(of: geometry.size) { _, size in viewportSize = size }
                })
            if showKeyboard {
                Divider()
                HStack {
                    Button { showKeyboard.toggle() } label: { Image(systemName: "keyboard") }.accessibilityLabel("文字入力")
                    Spacer()
                    Button("Tab") { send(.key(key: .tab)) }
                    Button { send(.key(key: .backspace)) } label: { Image(systemName: "delete.left") }
                        .accessibilityLabel("1文字削除")
                    Button("Enter") { send(.key(key: .enter)) }
                }.padding(12).disabled(!acceptsInput)
                HStack {
                    SecureField("選択した欄に入力", text: $text)
                        .textInputAutocapitalization(.never).disableAutocorrection(true)
                        .textFieldStyle(.roundedBorder).onSubmit(insertText).accessibilityIdentifier("browser.text")
                    Button("入力", action: insertText).disabled(text.isEmpty || !acceptsInput)
                        .accessibilityIdentifier("browser.type")
                }.padding(.horizontal).padding(.bottom, 8)
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .bottomBar) {
                Button { send(.back) } label: { Label("前のページ", systemImage: "chevron.left") }
                    .disabled(!acceptsInput || frame?.canGoBack != true)
                Spacer()
                Button { send(.forward) } label: { Label("次のページ", systemImage: "chevron.right") }
                    .disabled(!acceptsInput || frame?.canGoForward != true)
                Spacer()
                browserMenu.disabled(!acceptsInput)
            }
        }
        .sheet(isPresented: Binding(get: { credentials != nil }, set: {
            if !$0 {
                credentials = nil
            }
        })) {
            if let credentials {
                BrowserCredentialsSheet(credentials: credentials, send: send) { self.credentials = nil }
            }
        }
        .task(id: scenePhase == .active) {
            guard scenePhase == .active else { return }
            while !Task.isCancelled {
                let request = commands.isEmpty ? request(.read) : commands.removeFirst()
                await perform(request)
                do { try await Task.sleep(for: .milliseconds(connectionError == nil ? 150 : 1500)) } catch { return }
            }
        }
        .onDisappear { commands.removeAll(); text = "" }
    }

    private var browserMenu: some View {
        Menu {
            Button("ログイン状態・保存パスワード") { send(.credentials) }
                .accessibilityIdentifier("browser.credentials")
            Section("開いているタブ") {
                ForEach(frame?.tabs ?? [], id: \.id) { tab in
                    Button { send(.selectTab(id: tab.id)) } label: {
                        Label(tab.url == "about:blank" ? "新しいタブ" : (tab.title.isEmpty ? tab.url : tab.title),
                              systemImage: tab.id == frame?.tabId ? "checkmark" : "globe")
                    }
                    .accessibilityIdentifier(tab.id == frame?.tabId ? "browser.title" : "browser.tab.\(tab.id)")
                }
            }
            Button("新しいタブ") { send(.openTab(url: "about:blank")) }
            Button("タブを閉じる") {
                if let currentTab {
                    send(.closeTab(id: currentTab.id))
                }
            }

            Divider()
            Button("画面に合わせる") {
                if let action = browserFitAction(width: viewportSize.width, height: viewportSize.height,
                                                 currentWidth: frame?.width ?? 0,
                                                 currentHeight: frame?.height ?? 0) {
                    send(action)
                }
            }
            Button("デスクトップ · 1280 × 800") { send(.resize(width: 1280, height: 800)) }

            Button("モバイル · 390 × 844") { send(.resize(width: 390, height: 844)) }

            Button("URL をコピー") { UIPasteboard.general.string = currentTab?.url }
            Button("文字入力") { showKeyboard.toggle() }
        } label: { Label("ブラウザのメニュー", systemImage: "ellipsis") }
            .accessibilityLabel("ブラウザのメニュー").accessibilityIdentifier("browser.menu")
    }

    private func open() {
        do {
            let url = try browserAddressUrl(input: address)
            addressFocused = false
            send(.navigate(url: url))
        } catch { self.error = error.localizedDescription }
    }

    private func insertText() {
        guard !text.isEmpty else { return }
        let submitted = text
        text = ""
        send(.type(text: submitted))
    }

    private func send(_ action: BrowserAction) {
        guard commands.count < 64 else { return }
        error = nil
        credentials = nil
        switch action {
        case .importCookies, .importPasswords: notice = "ブラウザデータを取り込み中… 接続先のMacでOSのアクセス確認が出たら許可してください。"
        default: notice = nil
        }
        commands.append(request(action))
    }

    private func request(_ action: BrowserAction) -> BrowserRequest {
        BrowserRequest(threadId: thread, tabId: frame?.tabId ?? "", imageId: frame?.imageId ?? "",
                       generation: frame?.generation ?? 0, action: action)
    }

    private func perform(_ request: BrowserRequest) async {
        do {
            let result = try await store.browser(request: request)
            guard !Task.isCancelled, scenePhase == .active else { return }
            frame = result
            if let metadata = result.credentials {
                credentials = metadata
            }
            if !result.value.isEmpty {
                notice = result.value
            }
            if !result.image.isEmpty {
                image = UIImage(data: result.image)
            }
            if !addressFocused {
                address = currentTab?.url == "about:blank" ? "" : (currentTab?.url ?? "")
            }
            connectionError = nil
        } catch {
            guard !Task.isCancelled else { return }
            let message: String = if case let AgentError.Failed(reason) = error {
                reason
            } else {
                error.localizedDescription
            }
            if case .read = request.action {
                connectionError = message
            } else {
                self.error = message
            }
        }
    }
}

private struct BrowserCredentialsSheet: View {
    let credentials: BrowserCredentials
    let send: (BrowserAction) -> Void
    let close: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section("接続先のブラウザから取り込む") {
                    if credentials.profiles.isEmpty {
                        Text("取り込めるブラウザプロファイルがありません。")
                    }
                    ForEach(credentials.profiles, id: \.label) { profile in
                        VStack(alignment: .leading, spacing: 12) {
                            Text(profile.label)
                            Button("ログイン状態（Cookie）") { send(.importCookies(profile: profile.id)) }
                            Button("保存パスワード → Keychain") { send(.importPasswords(profile: profile.id)) }
                        }.disabled(profile.id.isEmpty).padding(.vertical, 4)
                    }
                }
                if let origin = credentials.origin {
                    Section("保存パスワード · \(origin)") {
                        if credentials.accounts.isEmpty {
                            Text(credentials.passwordError ?? "このサイトの保存パスワードはありません。")
                        }
                        ForEach(credentials.accounts, id: \.self) { username in
                            Button(username.isEmpty ? "ユーザー名なし" : username) {
                                send(.fillPassword(origin: origin, username: username))
                            }
                        }
                        Button("このサイトの保存パスワードを削除", role: .destructive) {
                            send(.deletePasswords(origin: origin))
                        }.disabled(credentials.accounts.isEmpty)
                    }
                }
            }
            .navigationTitle("ブラウザデータ").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる", action: close) } }
        }
    }
}

private struct RemoteBrowserCanvas: UIViewRepresentable {
    let image: UIImage
    let acceptsInput: Bool
    let send: (BrowserAction) -> Void

    func makeUIView(context _: Context) -> BrowserCanvasView {
        BrowserCanvasView()
    }

    func updateUIView(_ view: BrowserCanvasView, context _: Context) {
        view.display(image)
        view.send = acceptsInput ? send : nil
    }
}

private final class BrowserCanvasView: UIScrollView, UIScrollViewDelegate {
    let page = UIImageView()
    var send: ((BrowserAction) -> Void)? {
        didSet { pagePan.isEnabled = send != nil }
    }

    private lazy var pagePan = UIPanGestureRecognizer(target: self, action: #selector(dragged))
    private var previousSize = CGSize.zero

    init() {
        super.init(frame: .zero)
        delegate = self
        contentSize = page.bounds.size
        addSubview(page)
        panGestureRecognizer.minimumNumberOfTouches = 2
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
        pagePan.maximumNumberOfTouches = 1
        addGestureRecognizer(pagePan)
        backgroundColor = .secondarySystemBackground
        accessibilityLabel = "Host の BEX ブラウザ。ピンチで拡大、2本指で表示範囲を移動できます。"
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func display(_ image: UIImage) {
        let changedSize = page.image?.size != image.size
        page.image = image
        guard changedSize else { return }
        minimumZoomScale = 1
        setZoomScale(1, animated: false)
        page.frame = CGRect(origin: .zero, size: image.size)
        contentSize = image.size
        previousSize = .zero
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard bounds.size != previousSize, bounds.width > 0, page.bounds.width > 0 else { return }
        previousSize = bounds.size
        minimumZoomScale = bounds.width / page.bounds.width
        maximumZoomScale = max(2, minimumZoomScale * 4)
        setZoomScale(minimumZoomScale, animated: false)
    }

    override func gestureRecognizerShouldBegin(_ gesture: UIGestureRecognizer) -> Bool {
        if gesture === pagePan,
           pagePan.location(in: self).x - pagePan.translation(in: self).x < bounds.minX + 32 {
            return false
        }
        return super.gestureRecognizerShouldBegin(gesture)
    }

    func viewForZooming(in _: UIScrollView) -> UIView? {
        page
    }

    @objc private func tapped(_ gesture: UITapGestureRecognizer) {
        let point = gesture.location(in: page)
        guard page.bounds.contains(point) else { return }
        send?(.click(x: point.x, y: point.y))
    }

    @objc private func dragged(_ gesture: UIPanGestureRecognizer) {
        guard gesture.state == .ended else { return }
        let point = gesture.location(in: page)
        let delta = gesture.translation(in: page)
        send?(.scroll(x: max(0, min(page.bounds.maxX - 1, point.x)), y: max(0, min(page.bounds.maxY - 1, point.y)),
                      deltaX: max(-4096, min(4096, -delta.x)), deltaY: max(-4096, min(4096, -delta.y))))
    }
}
