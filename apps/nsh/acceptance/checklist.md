# Mac NSH acceptance checklist

The features of the original clients and the Mac scenario that checks each.
Run with `scripts/dev-env.sh just nsh-acceptance [scenario-filter...]`; the step
language is documented in `apps/nsh/src/acceptance/script.rs`.

Status: **covered** (a scenario checks it), **partial** (a scenario checks part of
it), **not yet** (applies to the Mac, no scenario yet), **n/a** (no Mac equivalent).

## iOS acceptance tests (`apps/mobile/iosApp/UITests`)

| iOS test | Status | Mac scenario / note |
|---|---|---|
| AccountUITests.testSimulatorSwitchesCodexAccountsAndForksConversation | not yet | account switch in the model picker, fork from a message |
| AccountUITests.testSimulatorAddsClaudeAccountAndKeepsCodexSelected | not yet | |
| AccountUITests.testSimulatorGoesBackFromAccountLoginAndCanStartAgain | not yet | |
| AccountUITests.testSimulatorSignsInDirectlyFromModelSettings | not yet | |
| AccountUITests.testSimulatorOpensAccountManagementFromSettingsAndModelPicker | partial | settings (account in providers), model-picker (account row) |
| AccountUITests.testSimulatorAutomaticallyShowsModelControlsInExistingAndRunningConversations | not yet | |
| AccountUITests.testSimulatorComposerOffersFastModelAndEffortBeforeMicrophone | not yet | effort and speed show in the model picker |
| ActivityUITests.testSimulatorSendsWithOneTapWhileReadingHistory | not yet | |
| ActivityUITests.testSimulatorComposerGrowsToFitWrappedDraft | not yet | screenshot evidence only |
| ActivityUITests.testSimulatorCanStartAConversationInAProject | covered | folded-work, approval, tool-panel |
| ActivityUITests.testSimulatorGroupsLiveCommandsBetweenCommentaryAndExpandsOnTap | partial | folded-work (completed turn; live `[groups]` not yet) |
| ActivityUITests.testSimulatorShowsRetryingStreamErrorThenRecovers | not yet | fixture `[retry]` |
| ActivityUITests.testSimulatorKeepsFailedWorkCollapsedWithVisibleTerminalError | not yet | fixture `[failed]` |
| ActivityUITests.testSimulatorKeepsInputRequestVisibleUntilResolved | covered | approval |
| ActivityUITests.testSimulatorSendsQuestionChoiceOnTap | covered | question-choice |
| ActivityUITests.testSimulatorCanSkipQuestion | not yet | fixture `[request] [skip]`, button スキップする |
| AttachmentsUITests.testSimulatorCanAddASecondPhoto | n/a | iOS photo picker; Mac attaches files (not yet) |
| AttachmentsUITests.testSimulatorCanAttachPhotosAndVideos | n/a | as above |
| AttachmentsUITests.testSimulatorRetriesPhotoUploadAfterStorageRecovery | not yet | upload retry applies to Mac file attachments |
| AttachmentsUITests.testSimulatorCanAttachDownloadAndPrepareAIEdit | not yet | |
| DeviceNavigationUITests.testSimulatorWorkbenchStartsEmptyAndOwnsOnlyOpenedTabs | covered | tool-panel |
| DeviceNavigationUITests.testSimulatorBrowserUsesOneSharedImportMenu | not yet | browser tool |
| DeviceNavigationUITests.testSimulatorNativeTerminalCopiesTextOnLongPress | n/a | long press; Mac terminal copy not yet |
| DeviceNavigationUITests.testSimulatorNativeTerminalPastesMultilineText | not yet | terminal tool |
| DeviceNavigationUITests.testSimulatorNativeTerminalDoesNotDuplicateQueryResponses | not yet | terminal tool |
| DeviceNavigationUITests.testSimulatorNativeTerminalRetainsShellAfterReopening | not yet | terminal tool |
| DeviceNavigationUITests.testSimulatorUsesNativeProjectDisclosureAndWorkspaceTree | partial | open-conversation (project disclosure), tool-panel (file tree) |
| DeviceNavigationUITests.testPhysicalDeviceCanPairWithManualPayload | n/a | physical device |
| DictationUITests.testSimulatorDictationContinuesPastThirtySecondsAndReachesHost | not yet | needs microphone permission |
| DictationUITests.testSimulatorDictationPermissionDenialPreservesDraftAndSend | not yet | needs microphone permission |
| HistoryUITests.testSimulatorReopensRunningLongHistoryWithoutBlankViewport | not yet | |
| HistoryUITests.testSimulatorKeepsSmallOlderScrollDuringLiveUpdate | not yet | `scroll` works; visibility is screenshot evidence only |
| HistoryUITests.testSimulatorRendersVisualizationAndReopensIt | not yet | fixture `[visualize]` |
| HistoryUITests.testSimulatorRendersMarkdownTableAndReopensIt | not yet | fixture `[markdown-table]` |
| HistoryUITests.testSimulatorKeepsDraftDuringLongMarkdownStreamAndReopensFinalText | not yet | fixture `[long-markdown]` |
| HistoryUITests.testSimulatorReopensCompletedHistoryCollapsed | partial | folded-work (collapsed after completion; reopening not yet) |
| ImagesUITests.testSimulatorShowsGeneratedImagesAndOpensFileLinksAfterReopening | not yet | fixture `[generated-images]` |
| ImagesUITests.testSimulatorOpensOnlyTheTappedImageAndSavesIt | not yet | fixture `[images]` |
| ImagesUITests.testSimulatorDisplaysImagesInMessagesAndMarkdownAfterReopening | not yet | |
| ModelSelectionUITests.testSimulatorModelPickerUsesNativeFormAndAgentPicker | covered | model-picker |
| NavigationUITests.testSimulatorRemovesHostAndRequiresPairingAfterRelaunch | not yet | environment settings |
| NavigationUITests.testSimulatorStartsOnListAndPreservesDetailOnForeground | partial | open-conversation; iOS list/detail navigation is n/a |
| NavigationUITests.testSimulatorOpensQrScannerWhileAddingHost | n/a | camera QR scanner; Mac pastes the connection info |
| NavigationUITests.testSimulatorUsesNativeHostNavigationAndPairingDismissal | not yet | environment settings "PCを追加" |
| NavigationUITests.testSimulatorSearchesFromBottomBarAndCreatesInCollapsedProject | not yet | sidebar "会話を検索" |
| NavigationUITests.testSimulatorUsesClaudeWithoutCodexAndRestoresConversation | not yet | runner has no `--without-codex` Host yet |
| NavigationUITests.testSimulatorRepeatedlyReopensTasksAndNewDraftsAfterBackNavigation | not yet | Mac switches conversations in the sidebar |
| PaginationUITests.testSimulatorOpensLongInterruptedHistoryAtLatestMessage | covered | long-history |
| PaginationUITests.testSimulatorLoadsEarlierHistoryOnTapAtTheReadingPosition | covered | long-history (position by screenshot) |
| PaginationUITests.testSimulatorLoadsLatestFiveTitlesPerProjectAndExpandsOneProject | not yet | fixture `/title-fixture` |
| PaginationUITests.testSimulatorPaginatesRecentProjectsAndUnassignedChats | not yet | fixture `/list-fixture` |
| SynchronizationUITests.testSimulatorShowsHistoryLoadingBeforeHostReplies | not yet | fixture `/hold-history-reads` |
| SynchronizationUITests.testSimulatorRetriesAFailedTaskOpenWithoutLosingItsDraft | not yet | fixture `/fail-next-history-read` |
| SynchronizationUITests.testSimulatorReconnectClearsHistoryFailureAndPreservesDraft | not yet | |
| SynchronizationUITests.testSimulatorUpdatesAnOpenConversationFromAnotherClient | not yet | fixture `/client-reply` |
| SynchronizationUITests.testSimulatorKeepsOpenTaskAndFetchesLatestReplyAfterForeground | not yet | Mac app reactivation |
| SynchronizationUITests.testSimulatorFetchesNewTaskAfterForeground | not yet | Mac app reactivation |
| SynchronizationUITests.testSimulatorFetchesNewTaskWhenReturningToList | n/a | the Mac sidebar is always shown |
| SynchronizationUITests.testSimulatorMarksUnseenCompletionUntilOpened | not yet | |
| SynchronizationUITests.testSimulatorShowsAcceptedAdditionalInputBeforeCodexProcessesIt | not yet | fixture `[delayed-input]` |
| TurnUITests.testSimulatorKeepsLatestVisibleAcrossRepeatedLongHistorySubmissions | not yet | |
| TurnUITests.testSimulatorSelectsPluginAndSkillFromComposer | not yet | |
| TurnUITests.testSimulatorSelectsAssistantTextInPlaceAndAddsOnlySelectionToDraft | not yet | the driver cannot select text yet |
| TurnUITests.testSimulatorOpensSideChatWithoutLosingOriginalDraft | not yet | side chat tool |
| TurnUITests.testSimulatorCopiesSelectedOrWholeUserMessageText | not yet | the driver cannot read the clipboard yet |
| TurnUITests.testSimulatorKeepsChatUnassignedAfterSendingAndReopening | partial | send-message (sending; reopening not yet) |
| TurnUITests.testSimulatorApprovalEditorAndDraftSurviveReconnect | not yet | |
| WorktreeUITests.testSimulatorMarksMergedWorktreesToTheRightOfRunningStatus | not yet | fixture `/merge-worktree/*` |
| WorktreeUITests.testSimulatorEditsHostWorktreeSettingsFromTaskMenu | partial | settings (opens the worktree section) |
| WorktreeUITests.testSimulatorReviewsTheOpenSessionsWorktree | partial | tool-panel (diff of the project) |

## GPUI desktop client (`apps/desktop`)

| Feature | Status | Mac scenario / note |
|---|---|---|
| Sidebar: projects, chats, new chat, new chat in a project | partial | send-message, open-conversation, folded-work |
| Sidebar: search, project menu, add/hide/rename project | not yet | |
| Conversation: messages, folded work, approvals, questions | covered | send-message, folded-work, approval, question-choice |
| Conversation: copy, edit in composer, fork from a message | not yet | |
| Composer: model picker | covered | model-picker |
| Composer: approval mode menu | covered | permission-mode |
| Composer: attach files, dictation, completions | not yet | |
| Thread details card (workspace, editor, changes, sources) | not yet | |
| Changed-files bar and review | partial | tool-panel (diff) |
| Tool panel: files, file preview, diff, close | covered | tool-panel |
| Tool panel: layout, browser, terminal, agents, side chat | not yet | |
| Settings: general, providers, environment, worktrees | covered | settings |
| Settings: display language | covered | display-language |
| Settings: accounts, updates, worktree editing | not yet | |
| Onboarding without a Host | not yet | |
| Keyboard shortcuts | not yet | `key` steps reach the keymap |
| Markdown tables, diagrams, images, file links | not yet | |
