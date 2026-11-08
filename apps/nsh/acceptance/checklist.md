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
| DeviceNavigationUITests.testSimulatorBrowserUsesOneSharedImportMenu | covered | browser (a page from the fixture Host, typing into it, tabs and the data import from one menu); the Mac page takes typing itself, so the iPhone's input row is n/a |
| DeviceNavigationUITests.testSimulatorNativeTerminalCopiesTextOnLongPress | n/a | long press |
| DeviceNavigationUITests.testSimulatorNativeTerminalPastesMultilineText | covered | terminal-paste |
| DeviceNavigationUITests.testSimulatorNativeTerminalDoesNotDuplicateQueryResponses | covered | terminal-query |
| DeviceNavigationUITests.testSimulatorNativeTerminalRetainsShellAfterReopening | covered | terminal-reopen (the shell keeps its variable while another tool is in front and the panel closes and opens; the draft stays), tools |
| DeviceNavigationUITests.testSimulatorUsesNativeProjectDisclosureAndWorkspaceTree | covered | open-conversation (project disclosure), tool-panel (file tree) |
| DeviceNavigationUITests.testPhysicalDeviceCanPairWithManualPayload | n/a | physical device |
| DictationUITests.testSimulatorDictationContinuesPastThirtySecondsAndReachesHost | not yet | recording needs the system's microphone permission, whose prompt reaches the user running the scenarios; the driver can only decline it |
| DictationUITests.testSimulatorDictationPermissionDenialPreservesDraftAndSend | covered | dictation-denied (`microphone deny`) |
| HistoryUITests.testSimulatorReopensRunningLongHistoryWithoutBlankViewport | not yet | needs a long conversation that is still running |
| HistoryUITests.testSimulatorKeepsSmallOlderScrollDuringLiveUpdate | not yet | `scroll` works; the reading position is screenshot evidence only |
| HistoryUITests.testSimulatorRendersVisualizationAndReopensIt | partial | visualization (the Mac offers the file and opens it in the tool panel; it does not render it inline) |
| HistoryUITests.testSimulatorRendersMarkdownTableAndReopensIt | covered | markdown-table |
| HistoryUITests.testSimulatorKeepsDraftDuringLongMarkdownStreamAndReopensFinalText | covered | long-markdown |
| HistoryUITests.testSimulatorReopensCompletedHistoryCollapsed | covered | folded-work, markdown-table (reopened collapsed) |
| ImagesUITests.testSimulatorShowsGeneratedImagesAndOpensFileLinksAfterReopening | partial | images (a file link opens in the tool panel); generated images are screenshot evidence only |
| ImagesUITests.testSimulatorOpensOnlyTheTappedImageAndSavesIt | partial | images (a tapped image opens in Quick Look, also when asked while the app is in the background); which image of several opens, and saving, not yet: the image menu that saves is not on the Mac yet |
| ImagesUITests.testSimulatorDisplaysImagesInMessagesAndMarkdownAfterReopening | covered | images |
| ModelSelectionUITests.testSimulatorModelPickerUsesNativeFormAndAgentPicker | covered | model-picker |
| NavigationUITests.testSimulatorRemovesHostAndRequiresPairingAfterRelaunch | not yet | owned by nsh-parity-host-onboarding: NSH has no way to remove a paired Host yet (GPUI's 接続を解除); the runner's `relaunch` is ready for it |
| NavigationUITests.testSimulatorStartsOnListAndPreservesDetailOnForeground | partial | open-conversation; iOS list/detail navigation and foregrounding are n/a |
| NavigationUITests.testSimulatorOpensQrScannerWhileAddingHost | not yet | owned by nsh-parity-host-onboarding, which builds QR pairing; until then the Mac pastes the connection info, and core's pairing errors already ask the PC for a new QR code |
| NavigationUITests.testSimulatorUsesNativeHostNavigationAndPairingDismissal | covered | environment-settings |
| NavigationUITests.testSimulatorSearchesFromBottomBarAndCreatesInCollapsedProject | covered | sidebar-search; new chats start in the collapsed project in most scenarios |
| NavigationUITests.testSimulatorUsesClaudeWithoutCodexAndRestoresConversation | covered | claude-only (`host without-codex`, `relaunch`) |
| NavigationUITests.testSimulatorRepeatedlyReopensTasksAndNewDraftsAfterBackNavigation | partial | markdown-table, send-message, images (reopen once each); repeated switching not yet |
| PaginationUITests.testSimulatorOpensLongInterruptedHistoryAtLatestMessage | covered | long-history |
| PaginationUITests.testSimulatorLoadsEarlierHistoryOnTapAtTheReadingPosition | covered | long-history (position by screenshot) |
| PaginationUITests.testSimulatorLoadsLatestFiveTitlesPerProjectAndExpandsOneProject | covered | list-pagination |
| PaginationUITests.testSimulatorPaginatesRecentProjectsAndUnassignedChats | covered | list-pagination |
| SynchronizationUITests.testSimulatorShowsHistoryLoadingBeforeHostReplies | covered | history-loading |
| SynchronizationUITests.testSimulatorRetriesAFailedTaskOpenWithoutLosingItsDraft | covered | history-retry |
| SynchronizationUITests.testSimulatorReconnectClearsHistoryFailureAndPreservesDraft | covered | reconnect (the environment's connection switch reconnects; the failed read shows its error until the next reconnect clears it) |
| SynchronizationUITests.testSimulatorUpdatesAnOpenConversationFromAnotherClient | covered | other-client |
| SynchronizationUITests.testSimulatorKeepsOpenTaskAndFetchesLatestReplyAfterForeground | covered | reconnect (the reply another client added arrives with the reconnect, the draft stays) |
| SynchronizationUITests.testSimulatorFetchesNewTaskAfterForeground | covered | other-client-new (the Mac stays connected behind other apps, so the new conversation arrives without a return) |
| SynchronizationUITests.testSimulatorFetchesNewTaskWhenReturningToList | n/a | the Mac sidebar is always shown |
| SynchronizationUITests.testSimulatorMarksUnseenCompletionUntilOpened | covered | unseen-completion |
| SynchronizationUITests.testSimulatorShowsAcceptedAdditionalInputBeforeCodexProcessesIt | covered | queued-input |
| TurnUITests.testSimulatorKeepsLatestVisibleAcrossRepeatedLongHistorySubmissions | not yet | the reading position is screenshot evidence only |
| TurnUITests.testSimulatorSelectsPluginAndSkillFromComposer | covered | composer (skills; the fixture lists no plugins) |
| TurnUITests.testSimulatorSelectsAssistantTextInPlaceAndAddsOnlySelectionToDraft | covered | message-actions |
| TurnUITests.testSimulatorOpensSideChatWithoutLosingOriginalDraft | covered | side-chat |
| TurnUITests.testSimulatorCopiesSelectedOrWholeUserMessageText | partial | message-actions (whole messages); copying a selection uses the system menu, whose titles follow the system language |
| TurnUITests.testSimulatorKeepsChatUnassignedAfterSendingAndReopening | covered | send-message |
| TurnUITests.testSimulatorApprovalEditorAndDraftSurviveReconnect | covered | relaunch-draft (approval, a saved edit in the diff, and the draft after `relaunch`) |
| WorktreeUITests.testSimulatorMarksMergedWorktreesToTheRightOfRunningStatus | covered | merged-worktrees |
| WorktreeUITests.testSimulatorEditsHostWorktreeSettingsFromTaskMenu | covered | worktree-settings (from the settings; the Mac has no task menu) |
| WorktreeUITests.testSimulatorReviewsTheOpenSessionsWorktree | covered | worktree-review, review-panel |

## GPUI desktop client (`apps/desktop`)

| Feature | Status | Mac scenario / note |
|---|---|---|
| Sidebar: projects, chats, new chat, new chat in a project | covered | send-message, open-conversation, folded-work, list-pagination |
| Sidebar: search, project menu, add/hide/rename project | covered | sidebar-search, projects |
| Sidebar: unseen completions, merged worktrees | covered | unseen-completion, merged-worktrees |
| New-chat hero: project choice, start without a project | covered | new-chat-hero |
| Conversation: messages, folded work, approvals, questions | covered | send-message, folded-work, live-work, approval, question-choice, question-skip |
| Conversation: errors, reconnecting, opening and its failure | covered | failed-turn, retry-stream, history-loading, history-retry |
| Conversation: copy, edit in composer, fork from a message, quote a selection | covered | message-actions, fork |
| Conversation: steering a running turn, minimap | covered | steering, queued-input |
| Composer: model picker | covered | model-picker |
| Composer: approval mode menu | covered | permission-mode |
| Composer: attach files, dictation, completions | partial | attachments, upload-failure, composer, dictation-denied; recording needs the system's microphone permission |
| Thread details card (workspace, editor, changes, sources) | partial | attachments (sources); the editor button opens an external app |
| Changed-files bar and review | covered | review-panel, worktree-review |
| Tool panel: files, file preview, diff, close | covered | tool-panel |
| Tool panel: layout, browser, terminal, agents, side chat | covered | review-panel (layout), tools, browser, terminal-paste, terminal-query, terminal-reopen, side-chat |
| Settings: general, providers, environment, worktrees | covered | settings, environment-settings |
| Settings: display language | covered | display-language |
| Settings: accounts, updates, worktree editing | partial | accounts (sign-in starts and stops), worktree-settings; completing a sign-in and updates not yet |
| Accessibility: the settings overlay is modal, message roles | covered | settings-modal, send-message |
| Onboarding without a Host | not yet | the runner always starts a fixture Host |
| Keyboard shortcuts | partial | composer (Return sends); the app binds no other shortcut (⌘, does nothing) |
| Markdown tables, diagrams, images, file links | partial | markdown-table, markdown-document, images, visualization; diagrams not yet |
