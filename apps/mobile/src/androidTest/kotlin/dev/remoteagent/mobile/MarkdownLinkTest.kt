package dev.remoteagent.mobile

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent as AndroidIntent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Surface
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Intent
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class MarkdownLinkTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun tapsOnWrappedWebsiteAndPullRequestLinksLaunchTheirExactUrls() {
        val launched = mutableListOf<AndroidIntent>()
        val text = "Android版を getbex.dev で公開しました。PR #65へ反映済みです。"
        show(
            "Android版を [getbex.dev](https://getbex.dev) で公開しました。[PR #65](https://github.com/ttizze/remote-agent/pull/65)へ反映済みです。",
            { launched.add(it) },
        )
        tapText(text, "getbex.dev")
        tapText(text, "PR #65")
        compose.runOnIdle {
            assertEquals(
                listOf("https://getbex.dev", "https://github.com/ttizze/remote-agent/pull/65"),
                launched.map { it.dataString },
            )
            assertTrue(launched.all { it.action == AndroidIntent.ACTION_VIEW })
            assertTrue(launched.all { AndroidIntent.CATEGORY_BROWSABLE in it.categories.orEmpty() })
        }
    }

    @Test
    fun linkInsideTableLaunchesTheBrowser() {
        val launched = mutableListOf<AndroidIntent>()
        show("| 配布 |\n| --- |\n| [Android APK](https://getbex.dev/#download) |", { launched.add(it) })
        tapText("Android APK", "Android APK")
        compose.runOnIdle { assertEquals("https://getbex.dev/#download", launched.single().dataString) }
    }

    @Test
    fun unavailableBrowserShowsTheUrlAndCopyAction() {
        show("[getbex.dev](https://getbex.dev)", { throw ActivityNotFoundException() })
        tapText("getbex.dev", "getbex.dev")
        compose.onNodeWithText("リンクを開けません").assertExists()
        compose.onNodeWithText("対応するブラウザーやアプリを確認してください。\nhttps://getbex.dev").assertExists()
        compose.onNodeWithText("リンクをコピー").performClick()
        compose.onNodeWithText("閉じる").performClick()
        compose.onNodeWithText("リンクを開けません").assertDoesNotExist()
    }

    @Test
    fun workspaceLinkStillUsesAuthenticatedFilePreview() {
        val launched = mutableListOf<AndroidIntent>()
        val intents = mutableListOf<Intent>()
        compose.setContent {
            val base = LocalContext.current
            CompositionLocalProvider(LocalContext provides LinkContext(base) { launched.add(it) }) {
                AppTheme {
                    ConversationBody("[notes.md](/repo/notes.md)", "/repo") { intent, complete ->
                        intents.add(intent)
                        complete(Result.failure(IOException("fixture preview")))
                    }
                }
            }
        }
        // File chips are inline replacements, so click their displayed label.
        compose.onNodeWithText("notes.md").performTouchInput { click() }
        compose.onNodeWithText("fixture preview").assertExists()
        compose.runOnIdle {
            assertTrue(launched.isEmpty())
            assertTrue(intents.any { it is Intent.DownloadFile })
        }
    }

    private fun show(markdown: String, launch: (AndroidIntent) -> Unit) {
        compose.setContent {
            val base = LocalContext.current
            CompositionLocalProvider(LocalContext provides LinkContext(base, launch)) {
                AppTheme { Surface { Column(Modifier.width(240.dp)) { ConversationBody(markdown) } } }
            }
        }
    }

    private fun tapText(paragraph: String, label: String) {
        val node = compose.onNodeWithText(paragraph)
        val layouts = mutableListOf<TextLayoutResult>()
        node.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
        val offset = paragraph.indexOf(label) + label.length / 2
        val position = layouts.single().getBoundingBox(offset).center
        node.performTouchInput { click(position) }
    }
}

private class LinkContext(base: Context, private val launch: (AndroidIntent) -> Unit) : ContextWrapper(base) {
    override fun startActivity(intent: AndroidIntent) = launch(intent)
}
