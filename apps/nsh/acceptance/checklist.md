# Mac NSH acceptance checklist

The features of the original clients and the Mac scenario that checks each.
Run with `scripts/dev-env.sh just nsh-acceptance [scenario-filter...]`; the step
language is documented in `apps/nsh/src/acceptance/script.rs`.

Status: **covered** (a scenario checks it), **partial** (a scenario checks part of
it; the note says what is missing), **not yet** (applies to the Mac, no scenario
yet; the note says why), **n/a** (no Mac equivalent).

## iOS acceptance tests (`apps/mobile/iosApp/UITests`)

| iOS test | Status | Mac scenario / note |
|---|---|---|
| AccountUITests.testSimulatorSwitchesCodexAccountsAndForksConversation | partial | fork; switching needs a second account, and the fixture's sign-in does not complete on the Mac (accounts stops it) |
| AccountUITests.testSimulatorAddsClaudeAccountAndKeepsCodexSelected | not yet | the fixture's sign-in does not complete on the Mac: after `account/login/start` the Host lists the same account |
| AccountUITests.testSimulatorGoesBackFromAccountLoginAndCanStartAgain | partial | accounts (Codex device-code sign-in stopped and started again); Claude's code entry not yet |
| AccountUITests.testSimulatorSignsInDirectlyFromModelSettings | not yet | as above, the sign-in does not complete |
| AccountUITests.testSimulatorOpensAccountManagementFromSettingsAndModelPicker | covered | settings, accounts, model-picker (account row opens the provider's settings) |
| AccountUITests.testSimulatorAutomaticallyShowsModelControlsInExistingAndRunningConversations | covered | live-work (model in a running conversation), long-history (sends in an existing one) |
| AccountUITests.testSimulatorComposerOffersFastModelAndEffortBeforeMicrophone | not yet | effort and speed show in the model picker only for the selected provider's model; no scenario reads them |
| ActivityUITests.testSimulatorSendsWithOneTapWhileReadingHistory | covered | long-history (sends after scrolling into earlier turns) |
| ActivityUITests.testSimulatorComposerGrowsToFitWrappedDraft | partial | composer (screenshot evidence only; the driver reads no sizes) |
| ActivityUITests.testSimulatorCanStartAConversationInAProject | covered | folded-work, approval, tool-panel |
| ActivityUITests.testSimulatorGroupsLiveCommandsBetweenCommentaryAndExpandsOnTap | covered | live-work |
| ActivityUITests.testSimulatorShowsRetryingStreamErrorThenRecovers | covered | retry-stream |
| ActivityUITests.testSimulatorKeepsFailedWorkCollapsedWithVisibleTerminalError | covered | failed-turn |
| ActivityUITests.testSimulatorKeepsInputRequestVisibleUntilResolved | covered | approval |
| ActivityUITests.testSimulatorSendsQuestionChoiceOnTap | covered | question-choice |
| ActivityUITests.testSimulatorCanSkipQuestion | covered | question-skip |
| AttachmentsUITests.testSimulatorCanAddASecondPhoto | n/a | iOS photo picker; the Mac attaches files (attachments) |
| AttachmentsUITests.testSimulatorCanAttachPhotosAndVideos | n/a | as above |
| AttachmentsUITests.testSimulatorRetriesPhotoUploadAfterStorageRecovery | covered | upload-failure (a file; the Mac attaches again rather than retrying in place) |
| AttachmentsUITests.testSimulatorCanAttachDownloadAndPrepareAIEdit | partial | attachments (attach, remove, send); the file viewer's download and AI edit not yet |
| DeviceNavigationUITests.testSimulatorWorkbenchStartsEmptyAndOwnsOnlyOpenedTabs | covered | tool-panel, tools |
| DeviceNavigationUITests.testSimulatorBrowserUsesOneSharedImportMenu | not yet | the browser's menu is disabled until a page loads, and loading one needs the network |
| DeviceNavigationUITests.testSimulatorNativeTerminalCopiesTextOnLongPress | n/a | long press |
| DeviceNavigationUITests.testSimulatorNativeTerminalPastesMultilineText | not yet | the terminal's screen is not in the accessibility tree, so the driver cannot read it |
| DeviceNavigationUITests.testSimulatorNativeTerminalDoesNotDuplicateQueryResponses | not yet | as above |
| DeviceNavigationUITests.testSimulatorNativeTerminalRetainsShellAfterReopening | partial | tools (opens and closes beside other tabs without failing); the shell is not readable |
| DeviceNavigationUITests.testSimulatorUsesNativeProjectDisclosureAndWorkspaceTree | covered | open-conversation (project disclosure), tool-panel (file tree) |
| DeviceNavigationUITests.testPhysicalDeviceCanPairWithManualPayload | n/a | physical device |
| DictationUITests.testSimulatorDictationContinuesPastThirtySecondsAndReachesHost | not yet | needs microphone permission |
| DictationUITests.testSimulatorDictationPermissionDenialPreservesDraftAndSend | not yet | needs microphone permission |
| HistoryUITests.testSimulatorReopensRunningLongHistoryWithoutBlankViewport | not yet | needs a long conversation that is still running |
| HistoryUITests.testSimulatorKeepsSmallOlderScrollDuringLiveUpdate | not yet | `scroll` works; the reading position is screenshot evidence only |
| HistoryUITests.testSimulatorRendersVisualizationAndReopensIt | partial | visualization (the Mac offers the file and opens it in the tool panel; it does not render it inline) |
| HistoryUITests.testSimulatorRendersMarkdownTableAndReopensIt | covered | markdown-table |
| HistoryUITests.testSimulatorKeepsDraftDuringLongMarkdownStreamAndReopensFinalText | covered | long-markdown |
| HistoryUITests.testSimulatorReopensCompletedHistoryCollapsed | covered | folded-work, markdown-table (reopened collapsed) |
| ImagesUITests.testSimulatorShowsGeneratedImagesAndOpensFileLinksAfterReopening | partial | images (a file link opens in the tool panel); generated images are screenshot evidence only |
| ImagesUITests.testSimulatorOpensOnlyTheTappedImageAndSavesIt | not yet | Quick Look shows only while the app is frontmost, which the runner does not force; it reports "このファイルをプレビューできませんでした。" there |
| ImagesUITests.testSimulatorDisplaysImagesInMessagesAndMarkdownAfterReopening | covered | images |
| ModelSelectionUITests.testSimulatorModelPickerUsesNativeFormAndAgentPicker | covered | model-picker |
| NavigationUITests.testSimulatorRemovesHostAndRequiresPairingAfterRelaunch | not yet | each scenario runs one app process; removing the only Host ends the scenario's connection |
| NavigationUITests.testSimulatorStartsOnListAndPreservesDetailOnForeground | partial | open-conversation; iOS list/detail navigation and foregrounding are n/a |
| NavigationUITests.testSimulatorOpensQrScannerWhileAddingHost | n/a | camera QR scanner; the Mac pastes the connection info |
| NavigationUITests.testSimulatorUsesNativeHostNavigationAndPairingDismissal | covered | environment-settings |
| NavigationUITests.testSimulatorSearchesFromBottomBarAndCreatesInCollapsedProject | covered | sidebar-search; new chats start in the collapsed project in most scenarios |
| NavigationUITests.testSimulatorUsesClaudeWithoutCodexAndRestoresConversation | not yet | the runner has no `--without-codex` Host |
| NavigationUITests.testSimulatorRepeatedlyReopensTasksAndNewDraftsAfterBackNavigation | partial | markdown-table, send-message, images (reopen once each); repeated switching not yet |
| PaginationUITests.testSimulatorOpensLongInterruptedHistoryAtLatestMessage | covered | long-history |
| PaginationUITests.testSimulatorLoadsEarlierHistoryOnTapAtTheReadingPosition | covered | long-history (position by screenshot) |
| PaginationUITests.testSimulatorLoadsLatestFiveTitlesPerProjectAndExpandsOneProject | covered | list-pagination |
| PaginationUITests.testSimulatorPaginatesRecentProjectsAndUnassignedChats | covered | list-pagination |
| SynchronizationUITests.testSimulatorShowsHistoryLoadingBeforeHostReplies | covered | history-loading |
| SynchronizationUITests.testSimulatorRetriesAFailedTaskOpenWithoutLosingItsDraft | covered | history-retry |
| SynchronizationUITests.testSimulatorReconnectClearsHistoryFailureAndPreservesDraft | not yet | needs the app to leave and regain the foreground |
| SynchronizationUITests.testSimulatorUpdatesAnOpenConversationFromAnotherClient | covered | other-client |
| SynchronizationUITests.testSimulatorKeepsOpenTaskAndFetchesLatestReplyAfterForeground | not yet | needs the app to leave and regain the foreground, which would take focus from the user |
| SynchronizationUITests.testSimulatorFetchesNewTaskAfterForeground | not yet | as above |
| SynchronizationUITests.testSimulatorFetchesNewTaskWhenReturningToList | n/a | the Mac sidebar is always shown |
| SynchronizationUITests.testSimulatorMarksUnseenCompletionUntilOpened | covered | unseen-completion |
| SynchronizationUITests.testSimulatorShowsAcceptedAdditionalInputBeforeCodexProcessesIt | covered | queued-input |
| TurnUITests.testSimulatorKeepsLatestVisibleAcrossRepeatedLongHistorySubmissions | not yet | the reading position is screenshot evidence only |
| TurnUITests.testSimulatorSelectsPluginAndSkillFromComposer | covered | composer (skills; the fixture lists no plugins) |
| TurnUITests.testSimulatorSelectsAssistantTextInPlaceAndAddsOnlySelectionToDraft | covered | message-actions |
| TurnUITests.testSimulatorOpensSideChatWithoutLosingOriginalDraft | covered | side-chat |
| TurnUITests.testSimulatorCopiesSelectedOrWholeUserMessageText | partial | message-actions (whole messages); copying a selection uses the system menu, whose titles follow the system language |
| TurnUITests.testSimulatorKeepsChatUnassignedAfterSendingAndReopening | covered | send-message |
| TurnUITests.testSimulatorApprovalEditorAndDraftSurviveReconnect | not yet | needs a reconnect |
| WorktreeUITests.testSimulatorMarksMergedWorktreesToTheRightOfRunningStatus | covered | merged-worktrees |
| WorktreeUITests.testSimulatorEditsHostWorktreeSettingsFromTaskMenu | covered | worktree-settings (from the settings; the Mac has no task menu) |
| WorktreeUITests.testSimulatorReviewsTheOpenSessionsWorktree | covered | worktree-review, review-panel |

## Desktop client features

| Feature | Status | Mac scenario / note |
|---|---|---|
| Sidebar: projects, chats, new chat, new chat in a project | covered | send-message, open-conversation, folded-work, list-pagination |
| Sidebar: search, project menu, add/hide/rename project | covered | sidebar-search, projects |
| Sidebar: unseen completions, merged worktrees | covered | unseen-completion, merged-worktrees |
| New-chat hero: project choice, start without a project | covered | new-chat-hero |
| Conversation: messages, folded work, approvals, questions | covered | send-message, folded-work, live-work, approval, question-choice, question-skip |
| Conversation: errors, reconnecting, opening and its failure | covered | failed-turn, retry-stream, history-loading, history-retry |
| Conversation: copy, edit in composer, fork from a message, quote a selection | covered | message-actions, fork |
| Conversation: steering a running turn, minimap beside the lane | covered | steering, queued-input, minimap |
| Composer: model picker | covered | model-picker |
| Composer: approval mode menu | covered | permission-mode |
| Composer: controls fit narrow windows, none cut off | covered | composer-narrow |
| Composer: attach files, dictation, completions | partial | attachments, upload-failure, composer; dictation needs microphone permission |
| Thread details card (workspace, editor, changes, sources) | partial | attachments (sources); the editor button opens an external app |
| Changed-files bar and review | covered | review-panel, worktree-review |
| Tool panel: files, file preview, diff, close | covered | tool-panel |
| Tool panel: layout, browser, terminal, agents, side chat | partial | review-panel (layout), tools, side-chat; the terminal's screen and a loaded page are not observable |
| Settings: general, providers, environment, worktrees | covered | settings, environment-settings |
| Settings: display language | covered | display-language |
| Settings: accounts, updates, worktree editing | partial | accounts (sign-in starts and stops), worktree-settings; completing a sign-in and updates not yet |
| Accessibility: the settings overlay is modal, message roles | covered | settings-modal, send-message |
| Onboarding without a Host | not yet | the runner always starts a fixture Host |
| Keyboard shortcuts | partial | composer (Return sends); the app binds no other shortcut (⌘, does nothing) |
| Markdown tables, diagrams, images, file links | partial | markdown-table, markdown-document, images, visualization; diagrams not yet |
