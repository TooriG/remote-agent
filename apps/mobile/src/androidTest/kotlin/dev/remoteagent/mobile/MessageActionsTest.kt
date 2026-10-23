package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.ClipboardManager
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.remoteagent.core.ConversationRowContent
import dev.remoteagent.core.OperationPhase
import dev.remoteagent.core.ProviderKind
import dev.remoteagent.core.SessionRef
import dev.remoteagent.core.Snapshot
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class MessageActionsTest {
    @get:Rule val compose = createComposeRule()

    private fun conversation(): Snapshot {
        val id = JSONObject("""{"provider":"codex","id":"actions"}""")
        val thread =
            JSONObject()
                .put("id", id)
                .put(
                    "capabilities",
                    JSONObject("""{"fork":true,"additionalInput":true,"rename":false,"modelChange":false}"""),
                )
                .put(
                    "turns",
                    JSONArray()
                        .put(
                            JSONObject()
                                .put("id", "turn")
                                .put("status", "completed")
                                .put(
                                    "items",
                                    JSONArray()
                                        .put(messageItem("user", "My original message", true))
                                        .put(messageItem("answer", "Assistant reply")),
                                )
                        ),
                )
        val state =
            JSONObject(Snapshot.empty().serialize().decodeToString())
                .put(
                    "navigation",
                    JSONObject()
                        .put("thread_id", id)
                        .put("draft_key", JSONObject().put("Session", JSONObject().put("session", id)))
                        .put("cwd", "/fixture"),
                )
                .put("conversations", JSONArray().put(JSONArray().put(id).put(thread)))
        return Snapshot.restore(state.toString().encodeToByteArray())
    }

    @Test
    fun forkPhaseShowsProgressPreventsRepeatAndAllowsRetryAfterFailure() {
        conversation().use { snapshot ->
            val projection =
                projectConversationRows(
                    snapshot,
                    snapshot.conversation(SessionRef(ProviderKind.CODEX, "actions")),
                    null,
                )
            var calls = 0
            val response = requireNotNull(projection).rows.single { it.content is ConversationRowContent.Response }
            var phase by mutableStateOf<OperationPhase?>(null)
            compose.setContent {
                MaterialTheme {
                    ThreadMessageCard(
                        (response.content as ConversationRowContent.Response).item,
                        false,
                        "/fixture",
                        { _, _ -> },
                        fork = {
                            calls++
                            phase = OperationPhase.Running
                        },
                        forkPhase = phase,
                    )
                }
            }
            compose.onNodeWithContentDescription("分岐").performClick()
            compose.onNodeWithText("分岐を作成中…").assertIsDisplayed()
            assertEquals(1, calls)
            // The action is disabled until the actual operation completes.
            compose.onAllNodesWithContentDescription("分岐").assertCountEquals(0)
            compose.runOnIdle { phase = OperationPhase.Failed("offline") }
            compose.onNodeWithText("分岐できませんでした。もう一度お試しください。").assertIsDisplayed()
            compose.onNodeWithContentDescription("分岐").performClick()
            assertEquals(2, calls)
            compose.onNodeWithText("分岐を作成中…").assertIsDisplayed()
            compose.runOnIdle { phase = null }
            compose.onNodeWithText("分岐を作成中…").assertDoesNotExist()
            compose.onNodeWithContentDescription("分岐").assertIsDisplayed()
        }
    }

    @Test
    fun userAndAssistantMessagesCopyExactTextAndAcknowledgeTheTap() {
        conversation().use { snapshot ->
            val projection =
                projectConversationRows(
                    snapshot,
                    snapshot.conversation(SessionRef(ProviderKind.CODEX, "actions")),
                    null,
                )
            lateinit var clipboard: ClipboardManager
            compose.setContent {
                clipboard = LocalClipboardManager.current
                MaterialTheme { ThreadDetailScreen(snapshot, projection, { _, _ -> }, null) {} }
            }
            compose.onAllNodesWithContentDescription("コピー").assertCountEquals(2)
            compose.onAllNodesWithContentDescription("コピー")[0].performClick()
            compose.onNodeWithContentDescription("コピーしました").assertIsDisplayed()
            compose.runOnIdle { assertEquals("My original message", clipboard.getText()?.text) }
            compose.onAllNodesWithContentDescription("コピー")[0].performClick()
            compose.runOnIdle { assertEquals("Assistant reply", clipboard.getText()?.text) }
        }
    }
}
