import XCTest

extension LaunchUITests {
    func testSimulatorModelDefaultsInheritAndPersistAcrossScopes() throws {
        let app = try connectedSimulatorApp()
        openDefaultModelSettings(app)
        let choice = app.buttons["model.choice.fixture-model"]
        XCTAssertTrue(choice.waitForExistence(timeout: 15)); choice.tap()
        chooseIndependentNewChatModel(app)
        chooseDefaultModelMenuOption(app, menu: "model.sheet.effort", option: "medium")
        chooseDefaultModelMenuOption(
            app,
            menu: "settings.scope.environment",
            option: "settings.scope.environment.current", towardTop: true
        )
        chooseDefaultModelMenuOption(app, menu: "model.sheet.effort", option: "high")
        chooseDefaultModelMenuOption(app, menu: "model.defaults.speed", option: "高速")
        chooseDefaultModelMenuOption(
            app,
            menu: "settings.scope.projects",
            option: "settings.scope.projects.simulator-project", towardTop: true
        )
        XCTAssertEqual(defaultModelButton(app, "model.sheet.effort").value as? String, "high")
        chooseDefaultModelMenuOption(app, menu: "model.sheet.effort", option: "medium")
        XCTAssertTrue(defaultModelButton(app, "model.defaults.inherit").exists)
        captureScreen(app, named: "Project model defaults with environment inheritance")
        defaultModelButton(app, "model.defaults.inherit").tap()
        XCTAssertEqual(defaultModelButton(app, "model.sheet.effort").value as? String, "high")
        XCTAssertFalse(app.buttons["model.defaults.inherit"].exists)
        dismissModelSettings(app); app.buttons["tasks.new.project.simulator-project"].tap()
        XCTAssertEqual(app.buttons["model.effort"].value as? String, "high")
        XCTAssertEqual(app.buttons["model.fast"].value as? String, "オン")
        app.buttons["BackButton"].tap()
        openDefaultModelSettings(app)
        chooseDefaultModelMenuOption(
            app,
            menu: "settings.scope.projects",
            option: "settings.scope.projects.simulator-project", towardTop: true
        )
        chooseDefaultModelMenuOption(app, menu: "model.sheet.effort", option: "medium")
        dismissModelSettings(app); app.buttons["tasks.new.project.simulator-project"].tap()
        app.buttons["model.settings"].tap(); XCTAssertEqual(
            defaultModelButton(app, "model.sheet.effort").value as? String,
            "high"
        )
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["tasks.menu"].waitForExistence(timeout: 20))
        openDefaultModelSettings(app)
        chooseDefaultModelMenuOption(
            app,
            menu: "settings.scope.projects",
            option: "settings.scope.projects.simulator-project", towardTop: true
        )
        XCTAssertEqual(defaultModelButton(app, "model.sheet.effort").value as? String, "medium")
    }

    func testSimulatorModelPickerUsesNativeFormAndAgentPicker() throws {
        let app = try connectedSimulatorApp()
        app.buttons["tasks.new.project.simulator-project"].tap()
        chooseFixtureModel(app)
        app.buttons["model.settings"].tap()
        XCTAssertFalse(app.buttons["settings.scope.projects"].exists || app.buttons["settings.scope.environment"]
            .exists)
        let weekly = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "週間残量 86%"))
            .firstMatch
        XCTAssertTrue(weekly.waitForExistence(timeout: 15)); XCTAssertTrue(weekly.isHittable)
        XCTAssertFalse(app.staticTexts["5時間枠"].exists || app.staticTexts["72%"].exists)
        let picker = app.segmentedControls["model.provider"]
        let codex = picker.buttons["Codex"]
        let claude = picker.buttons["Claude Code"]
        XCTAssertTrue(codex.isSelected)
        XCTAssertEqual(codex.frame.midY, claude.frame.midY, accuracy: 1)
        XCTAssertTrue(app.navigationBars["モデル設定"].exists)
        XCTAssertTrue(app.buttons["model.close"].exists)
        XCTAssertTrue(app.searchFields.firstMatch.exists)
        XCTAssertTrue(app.buttons["model.sheet.effort"].exists)
        XCTAssertTrue(app.buttons["model.sheet.fast"].exists)
        XCTAssertLessThan(app.buttons["model.sheet.effort"].frame.maxY,
                          app.buttons["model.sheet.fast"].frame.minY)
        captureScreen(app, named: "Native model form with agent picker and account usage")
        dismissModelSettings(app)
    }

    func testSimulatorModelDefaultsPersistAndApplyOnlyToNewConversations() throws {
        let app = try connectedSimulatorApp()
        app.buttons["tasks.new.project.simulator-project"].tap()
        chooseFixtureModel(app)
        chooseFixtureEffort(app, "medium")
        let fast = app.buttons["model.fast"]
        if fast.value as? String == "オン" {
            fast.tap()
        }
        XCTAssertEqual(fast.value as? String, "オフ")
        let prompt = app.textFields["task.message"]
        prompt.tap(); prompt.typeText("Keep this existing draft")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        openDefaultModelSettings(app)
        let choice = app.buttons["model.choice.fixture-model"]
        XCTAssertTrue(choice.waitForExistence(timeout: 15)); choice.tap()
        chooseIndependentNewChatModel(app)
        XCTAssertFalse(app.descendants(matching: .any)["model.error"].exists)
        chooseDefaultModelMenuOption(app, menu: "model.sheet.effort", option: "high")
        chooseDefaultModelMenuOption(app, menu: "model.defaults.speed", option: "高速")
        captureScreen(app, named: "Default model effort and speed for new conversations")
        dismissModelSettings(app)
        app.buttons["tasks.new.project.simulator-project"].tap()
        XCTAssertEqual(app.textFields["task.message"].value as? String, "Keep this existing draft")
        XCTAssertEqual(app.buttons["model.effort"].value as? String, "medium")
        XCTAssertEqual(app.buttons["model.fast"].value as? String, "オフ")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["tasks.new.chat"].tap()
        XCTAssertEqual(app.buttons["model.effort"].value as? String, "high")
        XCTAssertEqual(app.buttons["model.fast"].value as? String, "オン")
        app.buttons["model.settings"].tap()
        XCTAssertEqual(app.buttons["model.choice.fixture-model"].value as? String, "選択中")
        XCTAssertFalse(app.descendants(matching: .any)["model.error"].exists)
        dismissModelSettings(app)
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["tasks.menu"].waitForExistence(timeout: 20))
        openDefaultModelSettings(app)
        assertPersistedIndependentModelDefaults(app)
    }

    private func openDefaultModelSettings(_ app: XCUIApplication) {
        chooseDefaultModelMenuOption(app, menu: "tasks.menu", option: "tasks.settings")
        let models = app.buttons["settings.models"]
        XCTAssertTrue(models.waitForExistence(timeout: 10))
        waitForStableFrame(models)
        models.tap()
        XCTAssertTrue(app.buttons["model.choice.automatic"].waitForExistence(timeout: 10))
        selectFixtureProvider(app, "Codex")
    }

    private func defaultModelButton(_ app: XCUIApplication, _ id: String, towardTop: Bool = false) -> XCUIElement {
        let control = app.buttons[id]
        if towardTop {
            scrollToEarlierListElement(control, in: app, attempts: 6)
        } else {
            scrollToListElement(control, in: app, bottomMargin: 0)
        }
        return control
    }

    private func chooseDefaultModelMenuOption(
        _ app: XCUIApplication, menu: String, option: String, towardTop: Bool = false
    ) {
        tapWhenStable(defaultModelButton(app, menu, towardTop: towardTop))
        tapWhenStable(app.buttons[option])
    }

    private func assertPersistedIndependentModelDefaults(_ app: XCUIApplication) {
        XCTAssertEqual(app.buttons["model.choice.fixture-model"].value as? String, "選択中")
        XCTAssertEqual(defaultModelButton(app, "model.sheet.effort").value as? String, "high")
        XCTAssertEqual(defaultModelButton(app, "model.defaults.speed").value as? String, "高速")
        XCTAssertEqual(
            defaultModelButton(app, "model.defaults.new-chat", towardTop: true).value as? String,
            "Fixture Model"
        )
        app.buttons["model.choice.automatic"].tap()
        XCTAssertEqual(app.buttons["model.choice.automatic"].value as? String, "選択中")
        XCTAssertEqual(
            defaultModelButton(app, "model.defaults.new-chat", towardTop: true).value as? String,
            "Fixture Model"
        )
    }

    private func chooseIndependentNewChatModel(_ app: XCUIApplication) {
        let newChat = defaultModelButton(app, "model.defaults.new-chat", towardTop: true)
        chooseDefaultModelMenuOption(
            app,
            menu: "model.defaults.new-chat",
            option: "model.defaults.new-chat.fixture-model", towardTop: true
        )
        XCTAssertEqual(newChat.value as? String, "Fixture Model")
        app.segmentedControls["model.provider"].buttons["Claude Code"].tap()
        XCTAssertEqual(app.buttons["model.choice.automatic"].value as? String, "選択中")
        XCTAssertEqual(newChat.value as? String, "Fixture Model")
        app.segmentedControls["model.provider"].buttons["Codex"].tap()
        XCTAssertEqual(defaultModelButton(app, "model.choice.fixture-model").value as? String, "選択中")
    }
}
