package dev.remoteagent.mobile

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import dev.remoteagent.core.UiLanguage
import dev.remoteagent.core.localizedUiText

internal val LocalUiLanguage = staticCompositionLocalOf { UiLanguage.JAPANESE }

internal fun UiLanguage.text(source: String): String = localizedUiText(this, source)

@Composable
internal fun LanguageSettingsDialog(language: UiLanguage, select: (UiLanguage) -> Unit, dismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(language.text("表示言語")) },
        text = {
            androidx.compose.foundation.layout.Column {
                listOf(UiLanguage.ENGLISH to "English", UiLanguage.JAPANESE to "日本語").forEach { (value, label) ->
                    TextButton(onClick = { select(value) }) { Text("${if (value == language) "✓ " else ""}$label") }
                }
            }
        },
        confirmButton = { TextButton(onClick = dismiss) { Text(language.text("完了")) } },
    )
}
