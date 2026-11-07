import SwiftUI

public struct SettingsScopeItem: Identifiable, Decodable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

public struct SettingsScopeFields: View {
    let profiles: [SettingsScopeItem]
    let selectedProfileId: String?
    let disabled: Bool
    let select: (String) -> Void
    let project: Binding<String?>?
    let projects: [SettingsScopeItem]

    public init(profiles: [SettingsScopeItem], selectedProfileId: String?, disabled: Bool,
                select: @escaping (String) -> Void, project: Binding<String?>? = nil,
                projects: [SettingsScopeItem] = []) {
        self.profiles = profiles
        self.selectedProfileId = selectedProfileId
        self.disabled = disabled
        self.select = select
        self.project = project
        self.projects = projects
    }

    public var body: some View {
        Picker("環境", selection: Binding<String?>(get: { selectedProfileId }, set: {
            if let id = $0 {
                select(id)
            }
        })) {
            if selectedProfileId == nil {
                Text("環境を選択").tag(String?.none)
            }
            ForEach(profiles) { profile in
                Text(verbatim: profile.name).tag(Optional(profile.id))
            }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("settings.scope.environment")
        .disabled(disabled)
        if let project {
            Picker("プロジェクト", selection: project) {
                Text("すべてのプロジェクト").tag(String?.none)
                ForEach(projects) { project in
                    Text(verbatim: project.name).tag(Optional(project.id))
                }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("settings.scope.project")
            .disabled(disabled)
        } else {
            LabeledContent("プロジェクト", value: "すべてのプロジェクト")
        }
    }
}
