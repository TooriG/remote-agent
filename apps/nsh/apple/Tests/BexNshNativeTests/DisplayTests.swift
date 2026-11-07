@testable import BexNshNative
import NativeSettingsViews
import XCTest

final class DisplayTests: XCTestCase {
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

    func testWorkspacePreservesHostEditorSelectionBranchAndSources() throws {
        guard case let .workspace(summary) = try content("workspace") else { return XCTFail("Expected workspace") }
        XCTAssertEqual(summary.directory, "日本語 👋")
        XCTAssertTrue(summary.enabled)
        XCTAssertTrue(summary.hasDirectory)
        XCTAssertEqual(summary.branch, "feature/native")
        XCTAssertEqual(summary.additions, 7)
        XCTAssertEqual(summary.deletions, 2)
        XCTAssertEqual(summary.editor, "cursor")
        XCTAssertEqual(summary.editors[1].title, "Finder")
        XCTAssertEqual(summary.sources[0].path, "/fixture/日本語.txt")
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

    func testAgentsPreserveIdentityProgressHierarchyAndOptionalDetails() throws {
        guard case let .agents(rows) = try content("agents") else { return XCTFail("Expected agent rows") }
        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0].key, "root/active")
        XCTAssertEqual(rows[0].title, "日本語の確認 👋")
        XCTAssertEqual(rows[0].status, "実行中")
        XCTAssertTrue(rows[0].active)
        XCTAssertEqual(rows[0].depth, 2)
        XCTAssertEqual(rows[0].model, "fixture-model")
        XCTAssertEqual(rows[0].detail, "進捗 1/2")
        XCTAssertFalse(rows[1].active)
        XCTAssertNil(rows[1].model)
        XCTAssertNil(rows[1].detail)
    }

    func testDiffPreservesOldAndNewLineNumbersAndBinaryFiles() throws {
        guard case let .diff(files) = try content("diff") else { return XCTFail("Expected diff files") }
        XCTAssertEqual(files.count, 2)
        XCTAssertEqual(files[0].path, "日本語 👋.txt")
        XCTAssertEqual(files[0].additions, 1)
        XCTAssertEqual(files[0].deletions, 1)
        XCTAssertEqual(files[0].rows[1].old, 1)
        XCTAssertNil(files[0].rows[1].new)
        XCTAssertEqual(files[0].rows[2].new, 1)
        XCTAssertNil(files[0].rows[2].old)
        XCTAssertEqual(files[0].rows[2].kind, "+")
        XCTAssertEqual(files[0].rows[2].text, "+変更 👋")
        XCTAssertEqual(files[1].path, "image.png")
        XCTAssertNil(files[1].additions)
        XCTAssertNil(files[1].deletions)
        XCTAssertTrue(files[1].rows.isEmpty)
    }
}
