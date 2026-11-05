import AgentCore
import Foundation
import UIKit

extension AppViewModel {
    func connect(afterForeground: Bool = false) {
        if afterForeground {
            connectProfiles(afterForeground: true)
        } else if screen != .pairing, let profile = profiles.first(where: { $0.id == selectedProfileId }) {
            connect(profile)
        }
    }

    func connectProfiles(afterForeground: Bool = false) {
        for profile in profiles {
            connect(profile, afterForeground: afterForeground)
        }
    }

    private func connect(_ profile: HostProfile, afterForeground: Bool = false) {
        let storage = MobileSessionStorage(host: profile.id)
        let session = sessions.open(host: profile.id, storage: storage, platform: storage)
        observe(session, host: profile.id)
        guard afterForeground || (!session.snapshot().connected() && connections[profile.id] == nil) else { return }
        connections[profile.id]?.cancel()
        if selectedProfileId == profile.id {
            notice = nil
        }
        connections[profile.id] = Task { [weak self] in
            let started = ProcessInfo.processInfo.systemUptime
            do {
                let owner = try await session.ready()
                owner.recordConnectionEvent(phase: .uiConnectStart, value: afterForeground ? 1 : 0)
                _ = try await session.resume(ticket: profile.ticket)
                guard let self, !Task.isCancelled else { return }
                let elapsed = (ProcessInfo.processInfo.systemUptime - started) * 1_000_000
                owner.recordConnectionEvent(phase: .uiConnectReady, value: UInt64(elapsed))
                if selectedProfileId == profile.id {
                    publish(owner.snapshot())
                    notice = nil
                } else {
                    liveActivities.resendTokens(hostID: profile.id)
                }
            } catch {
                session.store()?.recordConnectionEvent(
                    phase: Task.isCancelled ? .uiConnectCancelled : .uiConnectFailed,
                    value: UInt64((ProcessInfo.processInfo.systemUptime - started) * 1_000_000)
                )
                guard let self, !Task.isCancelled else { return }
                if selectedProfileId == profile.id {
                    notice = error.localizedDescription
                }
            }
            guard !Task.isCancelled else { return }
            self?.connections.removeValue(forKey: profile.id)
        }
    }

    func observe(_ session: ClientSession, host: String) {
        guard observations[host] == nil else { return }
        observations[host] = Task { [weak self] in
            do {
                let owner = try await session.ready()
                guard !Task.isCancelled else { return }
                var previous = owner.snapshot()
                self?.receiveHost(previous, host: host, listChanged: true)
                while !Task.isCancelled {
                    _ = try await owner.nextSnapshot(previous: previous)
                    guard let self, !Task.isCancelled else { return }
                    let latest = owner.snapshot()
                    let changes = latest.changes(previous: previous)
                    receiveHost(latest, host: host, listChanged: changes.list)
                    if changes.reconnected {
                        liveActivities.resendTokens(hostID: host)
                        if selectedProfileId == host {
                            notice = nil
                        }
                    }
                    previous = latest
                }
            } catch {
                guard !Task.isCancelled else { return }
                if self?.selectedProfileId == host {
                    self?.notice = error.localizedDescription
                }
            }
        }
    }

    private func receiveHost(_ next: AgentCore.Snapshot, host: String, listChanged: Bool) {
        if selectedProfileId == host {
            publish(next)
        } else {
            updateProfile(next, host: host)
            if allProfiles, listChanged {
                updateTaskList()
            }
        }
    }
}
