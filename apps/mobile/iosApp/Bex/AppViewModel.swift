import AgentCore
import Combine
import Foundation
import Network
import UIKit

@MainActor
final class AppViewModel: ObservableObject {
    @Published private(set) var snapshot = AgentCore.Snapshot.empty()
    @Published var screen: AppScreen = .profiles
    @Published var sideChatRequest: SideChatRequest?
    @Published var composerFocusRequest: UUID?
    @Published var transferError: String?
    @Published var transferring = false
    @Published var isConnecting = false
    @Published var pairingError: String?
    @Published private(set) var pairingInvitation: Invitation?
    @Published var notice: String?
    @Published var profiles: [HostProfile] = []
    @Published private(set) var selectedProfileId: String?
    @Published private(set) var conversation: ConversationPresentation?
    private(set) var list: ThreadList?
    private(set) var models: [Model] = []
    private var presentationTask: Task<Void, Never>?
    private var pendingPresentation: ConversationPresentationInput?
    private var recentConversations: [ConversationPresentation] = []

    private(set) var session: ClientSession?
    var store: AgentStore? {
        session?.store()
    }

    var observation: Task<Void, Never>?
    var connection: Task<Void, Never>?
    private let pathMonitor = NWPathMonitor()
    let liveActivities = TaskLiveActivities()
    let panels = Panels()
    let workbenchInstance = UUID()
    var workbenchSideChats: [UInt64: SideChatRequest] = [:]

    init() {
        observeLiveActivityRequests()
        pathMonitor.pathUpdateHandler = { [weak self] path in
            let available = path.status == .satisfied
            Task { @MainActor [weak self] in
                guard let self, let owner = store else { return }
                await owner.networkChanged()
                if available, UIApplication.shared.applicationState == .active,
                   !isConnected, !isConnecting {
                    connect()
                }
            }
        }
        pathMonitor.start(queue: DispatchQueue(label: "agent.network-path"))
        do { profiles = try HostProfile.load() } catch { notice = error.localizedDescription }
        if let id = UserDefaults.standard.string(forKey: "bex.selected-host"),
           profiles.contains(where: { $0.id == id }) {
            selectProfile(id)
        }
    }

    deinit { pathMonitor.cancel() }

    func selectProfile(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        screen = .threads
        if selectedProfileId == id, store != nil {
            connect(); return
        }
        let old = detachStore()
        selectedProfileId = id
        UserDefaults.standard.set(id, forKey: "bex.selected-host")
        publish(AgentCore.Snapshot.empty())
        let owner = ClientSession.open(storage: MobileSessionStorage(host: id), previous: old, recentChats: false,
                                       clientBuild: Self.clientBuild, deviceName: UIDevice.current.name)
        session = owner
        observation = Task { [weak self] in
            do {
                let store = try await owner.ready()
                guard let self, session === owner, !Task.isCancelled else { return }
                publish(store.snapshot())
                observe(store, host: id)
                connect()
            } catch {
                guard let self, session === owner, !Task.isCancelled else { return }
                notice = error.localizedDescription
            }
        }
    }

    private static var clientBuild: UInt64 {
        UInt64(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0") ?? 0
    }

    func removeProfile(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        do {
            let remaining = profiles.filter { $0.id != id }
            try DeviceIdentity.remove(id)
            if selectedProfileId == id {
                let old = detachStore()
                selectedProfileId = nil
                UserDefaults.standard.removeObject(forKey: "bex.selected-host")
                notice = nil
                publish(AgentCore.Snapshot.empty())
                Task { [weak self] in
                    do { try await old?.shutdown() } catch { self?.notice = error.localizedDescription }
                }
            }
            profiles = remaining
            try HostProfile.save(profiles)
            screen = .profiles
        } catch { notice = error.localizedDescription }
    }

    private func detachStore() -> ClientSession? {
        connection?.cancel()
        observation?.cancel()
        let old = session
        old?.retire()
        session = nil
        recentConversations.removeAll()
        pendingPresentation = nil
        conversation = nil
        isConnecting = false
        return old
    }

    func openPairing() {
        persist()
        connection?.cancel()
        isConnecting = false
        pairingError = nil
        pairingInvitation = nil
        screen = .pairing
    }

    func dismissPairing() {
        connection?.cancel()
        isConnecting = false
        pairingInvitation = nil
        screen = .profiles
    }

    func preparePairing(_ contents: String) {
        guard !isConnecting else { return }
        pairingInvitation = nil
        do {
            pairingInvitation = try parseInvitation(contents: contents, now: UInt64(Date().timeIntervalSince1970))
            pairingError = nil
        } catch { pairingError = error.localizedDescription }
    }

    func confirmPairing() {
        guard !isConnecting, let invitation = pairingInvitation else { return }
        do {
            let id = try validateInvitation(invitation: invitation, now: UInt64(Date().timeIntervalSince1970))
            pairingError = nil
            screen = .pairing
            isConnecting = true
            connection?.cancel()
            connection = Task { [weak self] in
                do {
                    let owner = try await ClientSession.pair(
                        storage: MobileSessionStorage(host: id), previous: self?.session,
                        ticket: invitation.endpoint, invitation: invitation.invitation, recentChats: false,
                        clientBuild: Self.clientBuild, deviceName: UIDevice.current.name
                    )
                    guard let self, !Task.isCancelled else { try? await owner.shutdown(); return }
                    observation?.cancel()
                    let old = session
                    profiles.removeAll { $0.id == id }
                    profiles.append(HostProfile(id: id, name: invitation.hostName, ticket: invitation.endpoint))
                    try HostProfile.save(profiles)
                    try? await old?.shutdown()
                    UserDefaults.standard.set(id, forKey: "bex.selected-host")
                    selectedProfileId = id
                    session = owner
                    recentConversations.removeAll()
                    pendingPresentation = nil
                    conversation = nil
                    publish(owner.snapshot())
                    screen = .threads
                    pairingInvitation = nil
                    isConnecting = false
                    try await observe(owner.ready(), host: id)
                    perform(.loadHostName(LoadHostName()))
                } catch {
                    guard !Task.isCancelled else { return }
                    self?.isConnecting = false; self?.pairingError = error.localizedDescription
                }
            }
        } catch { pairingError = error.localizedDescription }
    }

    func perform(_ intent: Intent, completion: @escaping (Result<Outcome, Error>) -> Void = { _ in }) {
        guard let owner = session else {
            completion(.failure(CocoaError(.fileReadUnknown)))
            return
        }
        do {
            let receipt = try owner.dispatch(intent: intent)
            publish(owner.snapshot())
            let host = selectedProfileId
            Task { [weak self] in
                let result: Result<Outcome, Error>
                do { result = try await .success(receipt.wait()) } catch { result = .failure(error) }
                guard let self else { return }
                if selectedProfileId == host, session === owner {
                    publish(owner.snapshot())
                }
                completion(result)
            }
        } catch { completion(.failure(error)) }
    }
}

extension AppViewModel {
    func publish(_ next: AgentCore.Snapshot) {
        let changes = next.changes(previous: snapshot)
        guard changes.changed else { return }
        if let name = next.hostName(),
           let index = profiles.firstIndex(where: { $0.id == selectedProfileId }),
           profiles[index].name != name {
            profiles[index].name = name
            do { try HostProfile.save(profiles) } catch { notice = error.localizedDescription }
        }
        if changes.list {
            list = next.threadList()
            if list != nil {
                store?.recordConnectionEvent(phase: .listPublished, value: next.connected() ? 1 : 0)
            }
        }
        if changes.models {
            models = next.models()
        }
        migrateWorkbench(to: next)
        snapshot = next
        synchronizeLiveActivities(foreground: UIApplication.shared.applicationState == .active)
        if changes.conversation {
            projectConversation(next.conversationSource())
        }
        if let error = session?.persistenceError() {
            notice = error
        }
    }

    private func projectConversation(_ source: AgentCore.Thread?) {
        guard let source else {
            conversation = nil
            pendingPresentation = nil
            return
        }
        if conversation?.id != source.id() {
            conversation = recentConversations.first { $0.id == source.id() }
        }
        pendingPresentation = ConversationPresentationInput(source: source, snapshot: snapshot, host: selectedProfileId)
        guard presentationTask == nil else { return }
        presentationTask = Task { [weak self] in
            while let self, let input = pendingPresentation {
                pendingPresentation = nil
                let previous = conversation
                let rendered = await Task.detached(priority: .userInitiated) {
                    ConversationPresentation.project(input.source, snapshot: input.snapshot, previous: previous)
                }.value
                if selectedProfileId == input.host, snapshot.conversationUnchanged(other: input.snapshot) {
                    conversation = rendered
                    if rendered.id != nil {
                        recentConversations.removeAll { $0.id == rendered.id }
                        recentConversations.insert(rendered, at: 0)
                        if recentConversations.count > 3 {
                            recentConversations.removeLast()
                        }
                    }
                }
            }
            self?.presentationTask = nil
        }
    }

    func persist() {
        guard let owner = session else { return }
        Task { [weak self] in
            do { try await owner.flush() } catch {
                if self?.session === owner {
                    self?.notice = error.localizedDescription
                }
            }
        }
    }

    func persistBeforeBackground() async {
        do { try await session?.flush() } catch { notice = error.localizedDescription }
        await liveActivities.flush()
    }
}
