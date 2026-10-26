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
    @State private var error: String?
    @State private var connectionError: String?
    @State private var clientId = UUID().uuidString
    @State private var commands: [BrowserRequest] = []
    @FocusState private var addressFocused: Bool
    @State private var showKeyboard = false
    @State private var dialogText = ""

    private var ownsControl: Bool {
        frame?.controller == .you
    }

    private var acceptsInput: Bool {
        ownsControl && scenePhase == .active && connectionError == nil
    }

    private var currentTab: BrowserTab? {
        frame?.tabs.first { $0.id == frame?.tabId }
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    ForEach(frame?.tabs ?? [], id: \.id) { tab in
                        HStack(spacing: 8) {
                            Button(tab.url == "about:blank" ? "新しいタブ" : (tab.title.isEmpty ? tab.url : tab.title)) {
                                send(.selectTab(id: tab.id))
                            }.lineLimit(1)
                            Button { send(.closeTab(id: tab.id)) } label: { Image(systemName: "xmark").font(.caption2) }
                                .accessibilityLabel("タブを閉じる")
                        }
                        .font(.caption).padding(.horizontal, 10).padding(.vertical, 8)
                        .background(
                            tab.id == frame?.tabId ? Color.primary.opacity(0.08) : .clear,
                            in: RoundedRectangle(cornerRadius: 8)
                        )
                    }
                    Button { send(.openTab(url: "about:blank")) } label: { Image(systemName: "plus") }
                        .padding(8).accessibilityLabel("新しいタブ")
                }.disabled(!acceptsInput).padding(.horizontal, 8)
            }
            HStack(spacing: 12) {
                Button { send(.back) } label: { Image(systemName: "chevron.left") }
                    .accessibilityLabel("前のページ").disabled(!acceptsInput || frame?.canGoBack != true)
                Button { send(.forward) } label: { Image(systemName: "chevron.right") }
                    .accessibilityLabel("次のページ").disabled(!acceptsInput || frame?.canGoForward != true)
                Button { send(.reload) } label: { Image(systemName: "arrow.clockwise") }
                    .accessibilityLabel("再読み込み").disabled(!acceptsInput)
                TextField("検索または URL", text: $address)
                    .keyboardType(.webSearch).textInputAutocapitalization(.never).disableAutocorrection(true)
                    .textFieldStyle(.plain).focused($addressFocused).submitLabel(.go).onSubmit(open)
                    .disabled(!acceptsInput).accessibilityIdentifier("browser.address")
                Button(action: open) { Image(systemName: "arrow.right") }
                    .disabled(!acceptsInput).accessibilityLabel("開く").accessibilityIdentifier("browser.open")
            }.font(.subheadline).padding(12).background(.quaternary.opacity(0.25))
            HStack {
                Label(
                    ownsControl ? "あなたが操作中" : (frame?.controller == .anotherViewer ? "別の端末が操作中" : "AI が操作できます"),
                    systemImage: ownsControl ? "hand.point.up.left" : "sparkles"
                )
                .font(.caption).foregroundStyle(.secondary)
                Spacer(minLength: 4)
                Button(ownsControl ? "AI に操作を戻す" : "操作権を取得") {
                    send(ownsControl ? .releaseControl : .takeControl)
                }.font(.caption.weight(.medium)).disabled(frame == nil || connectionError != nil)
                    .accessibilityIdentifier("browser.control")
                Menu {
                    Button("デスクトップ · 1280 × 800") { send(.resize(width: 1280, height: 800)) }
                    Button("モバイル · 390 × 844") { send(.resize(width: 390, height: 844)) }
                    Button("URL をコピー") { UIPasteboard.general.string = currentTab?.url }
                } label: { Image(systemName: "ellipsis") }.disabled(frame == nil)
            }.padding(.horizontal, 12).padding(.vertical, 10)
            Divider()
            if let message = error ?? connectionError {
                BexNotice(text: message).padding(.horizontal)
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
                    VStack(spacing: 12) {
                        Image(systemName: "globe").font(.largeTitle)
                        Text("ブラウザで確認する").font(.headline)
                        Text("URL を開くか、AI にブラウザ操作を依頼してください。")
                            .font(.subheadline).multilineTextAlignment(.center)
                    }.foregroundStyle(.secondary).padding(24)
                } else {
                    ProgressView("Host のブラウザに接続中…")
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
            if ownsControl {
                Divider()
                HStack {
                    Button { showKeyboard.toggle() } label: { Image(systemName: "keyboard") }.accessibilityLabel("文字入力")
                    Spacer()
                    Button("Tab") { send(.key(key: .tab)) }
                    Button { send(.key(key: .backspace)) } label: { Image(systemName: "delete.left") }
                        .accessibilityLabel("1文字削除")
                    Button("Enter") { send(.key(key: .enter)) }
                }.padding(12).disabled(!acceptsInput)
                if showKeyboard {
                    HStack {
                        SecureField("選択した欄に入力", text: $text)
                            .textInputAutocapitalization(.never).disableAutocorrection(true)
                            .textFieldStyle(.roundedBorder).onSubmit(insertText).accessibilityIdentifier("browser.text")
                        Button("入力", action: insertText).disabled(text.isEmpty || !acceptsInput)
                            .accessibilityIdentifier("browser.type")
                    }.padding(.horizontal).padding(.bottom, 8)
                }
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
        .onChange(of: scenePhase) { phase in
            if phase != .active {
                releaseControl()
            }
        }
        .onDisappear { commands.removeAll(); text = ""; releaseControl() }
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
        commands.append(request(action))
    }

    private func request(_ action: BrowserAction) -> BrowserRequest {
        BrowserRequest(threadId: thread, tabId: frame?.tabId ?? "", imageId: frame?.imageId ?? "",
                       clientId: clientId, generation: frame?.generation ?? 0, action: action)
    }

    private func releaseControl() {
        guard ownsControl, let frame else { return }
        let request = BrowserRequest(threadId: thread, tabId: frame.tabId, imageId: "", clientId: clientId,
                                     generation: frame.generation, action: .releaseControl)
        Task { _ = try? await store.browser(request: request) }
    }

    private func perform(_ request: BrowserRequest) async {
        do {
            let result = try await store.browser(request: request)
            guard !Task.isCancelled, scenePhase == .active else { return }
            frame = result
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

private struct RemoteBrowserCanvas: UIViewRepresentable {
    let image: UIImage
    let acceptsInput: Bool
    let send: (BrowserAction) -> Void

    func makeUIView(context _: Context) -> BrowserCanvasView {
        BrowserCanvasView()
    }

    func updateUIView(_ view: BrowserCanvasView, context _: Context) {
        view.page.image = image
        if view.page.bounds.size != image.size {
            view.page.frame = CGRect(origin: .zero, size: image.size)
            view.contentSize = image.size
            view.setNeedsLayout()
        }
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
                      deltaX: -delta.x, deltaY: -delta.y))
    }
}
