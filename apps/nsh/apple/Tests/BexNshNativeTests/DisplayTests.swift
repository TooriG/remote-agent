@testable import BexNshNative
import XCTest

final class DisplayTests: XCTestCase {
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
