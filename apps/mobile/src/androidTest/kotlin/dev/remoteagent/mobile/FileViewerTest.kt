package dev.remoteagent.mobile

import androidx.compose.material3.Surface
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollTo
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import dev.remoteagent.core.FileContent
import dev.remoteagent.core.Intent
import dev.remoteagent.core.MarkdownFileKind
import dev.remoteagent.core.MarkdownFileReference
import dev.remoteagent.core.Outcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test

/** T3's file surfaces keep each read alive independently and scroll the rendered document. */
class FileViewerTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun openingAndClosingAChildKeepsItsParentMarkdownAndDoesNotDropTheChild() {
        val reads = mutableListOf<String>()
        show("Parent body\n\n[child.md](child.md)", reads)
        compose.onNodeWithText("Parent body").assertIsDisplayed()
        val device = UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        val chip = device.wait(Until.findObject(By.text("child.md")), NATIVE_MENU_WAIT_MILLIS)
        assertNotNull("native file chip", chip)
        chip!!.click()
        compose.waitForIdle()
        compose.onNodeWithText("Child body").assertIsDisplayed()
        compose.onNodeWithText("Parent body").assertExists()
        device.pressBack()
        compose.waitForIdle()
        compose.onNodeWithText("Child body").assertDoesNotExist()
        compose.onNodeWithText("Parent body").assertIsDisplayed()
        compose.runOnIdle { assertEquals(listOf("/repo/parent.md", "/repo/child.md"), reads) }
    }

    @Test
    fun renderedMarkdownCanScrollToItsLastParagraph() {
        show((1..100).joinToString("\n\n") { "Paragraph $it" } + "\n\nLast paragraph", mutableListOf())
        compose.onNodeWithText("Paragraph 1").assertIsDisplayed()
        compose.onNodeWithText("Last paragraph").assertIsNotDisplayed()
        compose.onNodeWithText("Last paragraph").performScrollTo().assertIsDisplayed()
    }

    private fun show(parent: String, reads: MutableList<String>) {
        compose.setContent {
            AppTheme {
                Surface {
                    ConversationFileViewer(
                        MarkdownFileReference("/repo/parent.md", "parent.md", MarkdownFileKind.MARKDOWN, null, null),
                        perform = { intent, complete ->
                            when (intent) {
                                is Intent.ResolveFileReference ->
                                    complete(Result.success(Outcome.FileReferenceResolved(intent.v1.path, false)))
                                is Intent.ReadFile -> {
                                    val path = intent.v1.path
                                    reads.add(path)
                                    val text = if (path.endsWith("parent.md")) parent else "Child body"
                                    complete(
                                        Result.success(
                                            Outcome.FileRead(FileContent(path, "r1", text, text.length.toULong()))
                                        )
                                    )
                                }
                                else -> error("unexpected viewer request: $intent")
                            }
                        },
                        dismiss = {},
                    )
                }
            }
        }
        compose.waitForIdle()
    }
}

private const val NATIVE_MENU_WAIT_MILLIS = 5_000L
