import AgentCore
import Combine
import Foundation
import Network
import UIKit

@MainActor
final class AppViewModel: ObservableObject {
    @Published private(set) var snapshot = AgentCore.Snapshot.empty()
    @Published var screen: AppScreen = .threads
    @Published var sideChatRequest: SideChatRequest?
    @Published var composerFocusRequest: UUID?
    @Published var transferError: String?
    @Published var transferring = false
    @Published var connections: [String: Task<Void, Never>] = [:]
    @Published var pairingConnection: Task<Void, Never>?
    var isConnecting: Bool {
        screen == .pairing ? pairingConnection != nil : selectedProfileId.flatMap { connections[$0] } != nil
    }

    @Published var pairingError: String?
    @Published private(set) var pairingInvitation: Invitation?
    private var pairingReturnScreen: AppScreen = .threads
    @Published var notice: String?
    @Published var profiles: [HostProfile] = []
    @Published private(set) var selectedProfileId: String?
    @Published private(set) var allProfiles = false
    @Published private(set) var conversation: ConversationPresentation?
    @Published private(set) var list: ThreadList?
    private(set) var models: [Model] = []
    private var presentationTask: Task<Void, Never>?
    private var pendingPresentation: ConversationPresentationInput?
    private var recentConversations: [ConversationPresentation] = []

    lazy var sessions = ClientSessions(recentChats: false, clientBuild: Self.clientBuild,
                                       deviceName: UIDevice.current.name)
    private(set) var session: ClientSession?

    var store: AgentStore? {
        session?.store()
    }

    var observations: [String: Task<Void, Never>] = [:]
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
                guard let self else { return }
                await sessions.networkChanged()
                if available, UIApplication.shared.applicationState == .active {
                    connectProfiles()
                }
            }
        }
        pathMonitor.start(queue: DispatchQueue(label: "agent.network-path"))
        do { profiles = try HostProfile.load() } catch { notice = error.localizedDescription }
        let selection = resolveConnectionSelection(hosts: profiles.map(\.id),
                                                   lastHost: UserDefaults.standard.string(forKey: "bex.selected-host"),
                                                   all: UserDefaults.standard.bool(forKey: "bex.all-hosts"))
        allProfiles = selection.all
        if let id = selection.hostId {
            activateProfile(id)
        }
        connectProfiles()
    }

    deinit { pathMonitor.cancel() }

    func selectProfile(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        allProfiles = false
        UserDefaults.standard.set(false, forKey: "bex.all-hosts")
        activateProfile(id)
        showThreadList()
        updateTaskList()
    }

    func selectAllProfiles() {
        allProfiles = true
        UserDefaults.standard.set(true, forKey: "bex.all-hosts")
        showThreadList()
        updateTaskList()
    }

    func activateProfile(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        if selectedProfileId == id, session != nil {
            return
        }
        screen = .threads
        resetPresentation()
        selectedProfileId = id
        UserDefaults.standard.set(id, forKey: "bex.selected-host")
        let storage = MobileSessionStorage(host: id)
        let owner = sessions.open(host: id, storage: storage, platform: storage)
        session = owner
        _ = try? owner.dispatch(intent: .showThreadList)
        publish(owner.snapshot())
        observe(owner, host: id)
    }

    func updateTaskList() {
        list = sessions.threadList(host: allProfiles ? nil : selectedProfileId)
    }

    private static var clientBuild: ClientBuild {
        ClientBuild(
            number: UInt64(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0") ?? 0,
            revision: Bundle.main.infoDictionary?["BEXBuildRevision"] as? String ?? "development"
        )
    }

    func removeProfile(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        do { try HostProfile.save(profiles.filter { $0.id != id }) } catch {
            notice = error.localizedDescription
            return
        }
        connections.removeValue(forKey: id)?.cancel()
        observations.removeValue(forKey: id)?.cancel()
        let old = sessions.remove(host: id)
        if selectedProfileId == id {
            resetPresentation()
            selectedProfileId = nil
            session = nil
            UserDefaults.standard.removeObject(forKey: "bex.selected-host")
        }
        profiles.removeAll { $0.id == id }
        let selection = resolveConnectionSelection(
            hosts: profiles.map(\.id),
            lastHost: selectedProfileId,
            all: allProfiles
        )
        allProfiles = selection.all
        UserDefaults.standard.set(allProfiles, forKey: "bex.all-hosts")
        if let host = selection.hostId {
            activateProfile(host)
        }
        updateTaskList()
        screen = .profiles
        Task { [weak self] in
            do {
                do { try await old?.shutdown() } catch { self?.notice = error.localizedDescription }
                try DeviceIdentity.remove(id)
                try? FileManager.default.removeItem(at: MobileSessionStorage(host: id).conversations())
            } catch { self?.notice = error.localizedDescription }
        }
    }

    private func resetPresentation() {
        recentConversations.removeAll()
        pendingPresentation = nil
        conversation = nil
        notice = nil
        publish(AgentCore.Snapshot.empty())
    }

    func openPairing() {
        if screen != .pairing {
            pairingReturnScreen = screen == .profiles ? .profiles : .threads
        }
        persist()
        pairingConnection?.cancel()
        pairingConnection = nil
        pairingError = nil
        pairingInvitation = nil
        screen = .pairing
    }

    func dismissPairing(returnTo: AppScreen? = nil) {
        pairingConnection?.cancel()
        pairingConnection = nil
        pairingInvitation = nil
        screen = returnTo ?? pairingReturnScreen
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
            pairingConnection = Task { [weak self] in
                guard let self else { return }
                do {
                    let storage = MobileSessionStorage(host: id)
                    let owner = try await sessions.pair(storage: storage, platform: storage, invitation: invitation)
                    guard !Task.isCancelled else { try? await owner.shutdown(); return }
                    let paired = profiles.filter { $0.id != id } + [
                        HostProfile(id: id, name: invitation.hostName, ticket: invitation.endpoint)
                    ]
                    try HostProfile.save(paired)
                    connections.removeValue(forKey: id)?.cancel()
                    observations.removeValue(forKey: id)?.cancel()
                    sessions.adopt(host: id, session: owner)
                    if selectedProfileId == id {
                        session = nil
                    }
                    guard !Task.isCancelled else { return }
                    profiles = paired
                    pairingInvitation = nil
                    pairingConnection = nil
                    selectProfile(id)
                } catch {
                    guard !Task.isCancelled else { return }
                    pairingConnection = nil
                    pairingError = error.localizedDescription
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
        updateProfile(next, host: selectedProfileId)
        if changes.list {
            updateTaskList()
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

    func updateProfile(_ next: AgentCore.Snapshot, host: String?) {
        // The Host's latest addresses let the next cold connection dial it directly.
        if let index = profiles.firstIndex(where: { $0.id == host }) {
            var profile = profiles[index]
            profile.name = next.hostName() ?? profile.name
            profile.ticket = next.hostTicket() ?? profile.ticket
            if profile != profiles[index] {
                profiles[index] = profile
                do { try HostProfile.save(profiles) } catch { notice = error.localizedDescription }
            }
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
        Task { [weak self] in
            guard let self else { return }
            do { try await sessions.flush() } catch { notice = error.localizedDescription }
        }
    }

    func persistBeforeBackground() async {
        do { try await sessions.flush() } catch { notice = error.localizedDescription }
        await liveActivities.flush()
    }
}
