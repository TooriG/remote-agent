package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.platform.ClipboardManager
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
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
        val thread = JSONObject().put("id", id)
            .put("capabilities", JSONObject("""{"fork":true,"additionalInput":true,"rename":false,"modelChange":false}"""))
            .put("turns", JSONArray().put(JSONObject().put("id", "turn").put("status", "completed")
                .put("items", JSONArray().put(messageItem("user", "My original message", true))
                    .put(messageItem("answer", "Assistant reply")))))
        val state = JSONObject(Snapshot.empty().serialize().decodeToString())
            .put("navigation", JSONObject().put("thread_id", id)
                .put("draft_key", JSONObject().put("Session", JSONObject().put("session", id))).put("cwd", "/fixture"))
            .put("conversations", JSONArray().put(JSONArray().put(id).put(thread)))
        return Snapshot.restore(state.toString().encodeToByteArray())
    }

    @Test
    fun forkShowsImmediateProgressPreventsRepeatAndAllowsRetryAfterFailure() {
        conversation().use { snapshot ->
            val projection = projectConversationRows(snapshot, snapshot.conversation(SessionRef(ProviderKind.CODEX, "actions")), null)
            var calls = 0
            var completion: ((Result<Outcome>) -> Unit)? = null
            compose.setContent {
                MaterialTheme {
                    ThreadDetailScreen(snapshot, projection, { intent, done ->
                        if (intent is Intent.ForkSession) {
                            calls++
                            completion = done
                        }
                    }, null) {}
                }
            }
            compose.onNodeWithContentDescription("??").performClick()
            compose.onNodeWithText("???????").assertIsDisplayed()
            assertEquals(1, calls)
            // The action is disabled until the actual operation completes.
            compose.onAllNodesWithContentDescription("??").assertCountEquals(0)
            compose.runOnIdle { completion!!(Result.failure(IllegalStateException("offline"))) }
            compose.onNodeWithText("???????????????????????").assertIsDisplayed()
            compose.onNodeWithContentDescription("??").performClick()
            assertEquals(2, calls)
            compose.onNodeWithText("???????").assertIsDisplayed()
            compose.runOnIdle { completion!!(Result.success(Outcome.Applied)) }
            compose.onNodeWithText("???????").assertDoesNotExist()
            compose.onNodeWithContentDescription("??").assertIsDisplayed()
        }
    }

    @Test
    fun userAndAssistantMessagesCopyExactTextAndAcknowledgeTheTap() {
        conversation().use { snapshot ->
            val projection = projectConversationRows(snapshot, snapshot.conversation(SessionRef(ProviderKind.CODEX, "actions")), null)
            lateinit var clipboard: ClipboardManager
            compose.setContent {
                clipboard = LocalClipboardManager.current
                MaterialTheme { ThreadDetailScreen(snapshot, projection, { _, _ -> }, null) {} }
            }
            compose.onAllNodesWithContentDescription("???").assertCountEquals(2)
            compose.onAllNodesWithContentDescription("???")[0].performClick()
            compose.onNodeWithContentDescription("???????").assertIsDisplayed()
            compose.runOnIdle { assertEquals("My original message", clipboard.getText()?.text) }
            compose.onAllNodesWithContentDescription("???")[0].performClick()
            compose.runOnIdle { assertEquals("Assistant reply", clipboard.getText()?.text) }
        }
    }
}
