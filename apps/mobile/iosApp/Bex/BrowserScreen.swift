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
    @State private var viewportSize = CGSize.zero
    @State private var followSize = true

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
            if (frame?.tabs.count ?? 0) > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(frame?.tabs ?? [], id: \.id) { tab in
                            HStack(spacing: 8) {
                                Button(tab.url == "about:blank" ? "新しいタブ" : (tab.title.isEmpty ? tab.url : tab.title)) {
                                    send(.selectTab(id: tab.id))
                                }.lineLimit(1)
                                    .accessibilityIdentifier(tab.id == frame?
                                        .tabId ? "browser.title" : "browser.tab.\(tab.id)")
                                Button { send(.closeTab(id: tab.id)) } label: {
                                    Image(systemName: "xmark").font(.caption2)
                                }
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
            }
            HStack(spacing: 0) {
                Button { send(.back) } label: { Image(systemName: "arrow.left").frame(width: 36, height: 44) }
                    .accessibilityLabel("前のページ").disabled(!acceptsInput || frame?.canGoBack != true)
                Button { send(.forward) } label: { Image(systemName: "arrow.right").frame(width: 36, height: 44) }
                    .accessibilityLabel("次のページ").disabled(!acceptsInput || frame?.canGoForward != true)
                Button { send(.reload) } label: { Image(systemName: "arrow.clockwise").frame(width: 36, height: 44) }
                    .accessibilityLabel("再読み込み").disabled(!acceptsInput)
                TextField("検索または URL", text: $address)
                    .keyboardType(.webSearch).textInputAutocapitalization(.never).disableAutocorrection(true)
                    .textFieldStyle(.plain).focused($addressFocused).submitLabel(.go).onSubmit(open)
                    .disabled(!acceptsInput).accessibilityIdentifier("browser.address")
                Menu {
                    Button("新しいタブ") { send(.openTab(url: "about:blank")) }.disabled(!acceptsInput)
                    Button("タブを閉じる") {
                        if let currentTab {
                            send(.closeTab(id: currentTab.id))
                        }
                    }
                    .disabled(!acceptsInput)
                    Divider()
                    Button("画面に合わせる（自動）") { followSize = true }.disabled(!acceptsInput)
                    Button("デスクトップ · 1280 × 800") { send(.resize(width: 1280, height: 800)) }
                        .disabled(!acceptsInput)
                    Button("モバイル · 390 × 844") { send(.resize(width: 390, height: 844)) }
                        .disabled(!acceptsInput)
                    Button("URL をコピー") { UIPasteboard.general.string = currentTab?.url }
                } label: { Image(systemName: "ellipsis").frame(width: 36, height: 44) }.disabled(frame == nil)
            }.font(.system(size: 14)).foregroundStyle(.secondary).padding(.horizontal, 4)
            Divider().opacity(0.6)
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
                    VStack(spacing: 24) {
                        Image(systemName: "globe").font(.system(size: 18))
                            .foregroundStyle(.secondary).frame(width: 36, height: 36)
                            .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
                            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.12)))
                            .padding(.bottom, 24)
                        Text("プレビューはまだありません").font(.system(size: 20, weight: .semibold))
                        Text("上の欄に URL を入力するか、AI にブラウザ操作を依頼してください。")
                            .font(.system(size: 14)).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }.frame(maxWidth: 384).padding(.horizontal, 24).padding(.top, 48)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                } else {
                    ProgressView("Host のブラウザに接続中…")
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(GeometryReader { geometry in
                    Color.clear.onAppear { viewportSize = geometry.size }
                        .onChange(of: geometry.size) { _, size in viewportSize = size }
                })
            Divider().opacity(0.6)
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
            }.padding(.horizontal, 12).padding(.vertical, 6)
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
                let fit = followSize && ownsControl ? browserFitAction(
                    width: viewportSize.width,
                    height: viewportSize.height,
                    currentWidth: frame?.width ?? 0,
                    currentHeight: frame?.height ?? 0
                ) : nil
                let request = commands.isEmpty ? request(fit ?? .read) : commands.removeFirst()
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
        if case .resize = action {
            followSize = false
        }
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
