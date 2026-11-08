@testable import BexNshNative
import NativeSettingsViews
import XCTest

final class DisplayTests: XCTestCase {
    func testTaskListKeepsProjectChildrenNativeActionsAndAccountScope() throws {
        guard case let .taskList(data) = try content("task-list")
        else { return XCTFail("Expected task list") }
        XCTAssertEqual(data.groups.count, 3)
        let project = try XCTUnwrap(data.groups.first)
        XCTAssertEqual(project.row.title, "日本語 👋")
        XCTAssertEqual(project.row.monogram, "日")
        XCTAssertEqual(project.row.color, 0x123456)
        XCTAssertTrue(project.row.expanded)
        XCTAssertEqual(project.row.compose?.enabled, false)
        XCTAssertTrue(try XCTUnwrap(project.row.compose?.id).contains("scope"))
        XCTAssertEqual(project.children.count, 1)
        XCTAssertEqual(project.children[0].indicators?.activity, "waiting")
        XCTAssertEqual(project.children[0].indicators?.worktree, "unmerged")
        XCTAssertEqual(project.children[0].statusLabel, "確認待ち、main に未反映の変更あり")
        XCTAssertEqual(data.groups[1].row.title, "チャット")
        XCTAssertTrue(data.groups[1].row.group)
        XCTAssertTrue(data.groups[2].children.isEmpty)
        XCTAssertNotEqual(project.children[0].id, data.groups[2].id)
        XCTAssertEqual(data.menu[0].children?[0].id, "restore 👋")
    }

    func testModelPickerPreservesCoreSearchProviderSelectionAndAccountQuota() throws {
        guard case let .modelPicker(data) = try content("model-picker")
        else { return XCTFail("Expected model picker") }
        let value = try XCTUnwrap(data)
        XCTAssertEqual(value.provider, "codex")
        XCTAssertEqual(value.query, "日本語")
        XCTAssertEqual(value.models.map(\.id), ["codex/native"])
        XCTAssertEqual(value.models[0].title, "日本語モデル 👋")
        XCTAssertEqual(value.selectedModel, "codex/native")
        XCTAssertEqual(value.customModel, "日本語")
        XCTAssertEqual(value.quick?.effort, "medium")
        XCTAssertEqual(value.quick?.speed, "priority")
        XCTAssertEqual(value.account?.title, "native@example.invalid")
        XCTAssertEqual(value.account?.usage[0].remainingPercent, 42)
        XCTAssertFalse(value.disabled)
    }

    func testWorktreeSettingsPreserveHostScopeOverridesAndCoreRemovalReason() throws {
        guard case let .worktreeEditor(data) = try content("worktree-editor")
        else { return XCTFail("Expected worktree editor") }
        let value = try XCTUnwrap(data)
        XCTAssertEqual(value.editor.host, "remote")
        XCTAssertEqual(value.hosts[0].name, "検証環境 👋")
        XCTAssertEqual(value.projects[0].id, "project")
        let settings = try XCTUnwrap(value.editor.settings)
        XCTAssertTrue(settings.createOnNewSession)
        XCTAssertEqual(settings.copyPathsText, ".env\nconfig/日本語")
        XCTAssertEqual(settings.projectDeleteUnchanged, ["project": true])
        let row = try XCTUnwrap(value.rows?.first)
        XCTAssertEqual(row.threads[0].id.provider, "codex")
        XCTAssertTrue(row.threads[0].active)
        XCTAssertEqual(row.reason, "現在開いている会話の作業場所です。別の会話に移動してから削除してください。")
    }

    func testNativeWorktreeActionsPreserveRawTextUntilCoreValidatesIt() throws {
        let settings = WorktreeSettingsInputs(
            createOnNewSession: true, copyOnCreate: true, copyPaths: ["  .env ", "", "config/日本語"],
            worktreeDirectory: "  /fixture/work trees  ", projectDeleteUnchanged: ["project": true]
        )
        let actions: [WorktreeAction] = [
            .save(id: "submission", settings: settings), .open(thread: WorktreeSession(provider: "codex", id: "chat")),
            .remove(path: "/fixture/work trees/作業"), .load, .close, .host(id: "remote"),
            .review(path: "/fixture/work trees/作業")
        ]
        let url = try XCTUnwrap(Bundle.module.url(forResource: "worktree-actions", withExtension: "json"))
        let expected = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? NSArray
        let encoded = try JSONEncoder().encode(actions)
        XCTAssertEqual(try JSONSerialization.jsonObject(with: encoded) as? NSArray, expected)
    }

    func testProjectEditorPreservesNativeInputAndCoreValidation() throws {
        guard case let .projectEditor(editor) = try content("project-editor")
        else { return XCTFail("Expected project editor") }
        let value = try XCTUnwrap(editor)
        XCTAssertEqual(value.id, "fixture-editor")
        XCTAssertEqual(value.name, "日本語のプロジェクト 👋")
        XCTAssertEqual(value.section, "個人")
        XCTAssertEqual(value.error, "表示名は1〜100文字、セクションは60文字以内で入力してください")
    }

    func testBrowserPreservesNativePromptAndHostCredentialMetadata() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "browser", withExtension: "json"))
        let content = try BrowserData(data: Data(contentsOf: url))
        XCTAssertTrue(content.enabled)
        XCTAssertEqual(content.image, "")
        XCTAssertEqual(content.dialog?.message, "入力してください 👋")
        XCTAssertEqual(content.dialog?.prompt, true)
        let credentials = try XCTUnwrap(content.credentials)
        XCTAssertEqual(credentials.profiles[0].id, "safari.test")
        XCTAssertEqual(credentials.profiles[0].label, "検証プロファイル 👋")
        XCTAssertEqual(credentials.importErrors[0].message, "権限を確認してください。")
        XCTAssertEqual(credentials.origin, "https://example.invalid")
        XCTAssertEqual(credentials.accounts, ["検証ユーザー"])
        XCTAssertNil(credentials.passwordError)
    }

    /// Rust asserts its actual encoder produces these same fixtures.
    private func content(_ name: String) throws -> DetailsContent {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json"))
        return try JSONDecoder().decode(DetailsContent.self, from: Data(contentsOf: url))
    }
}
