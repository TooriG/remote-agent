package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val BexColors =
    darkColorScheme(
        primary = Color(0xFFB5A0F5),
        onPrimary = Color(0xFF1D1532),
        primaryContainer = Color(0xFF352B4D),
        onPrimaryContainer = Color(0xFFE9DFFF),
        background = Color(0xFF111316),
        onBackground = Color(0xFFF0F1F3),
        surface = Color(0xFF111316),
        onSurface = Color(0xFFF0F1F3),
        surfaceVariant = Color(0xFF20242B),
        onSurfaceVariant = Color(0xFF989DA8),
        outline = Color(0xFF565D69),
        outlineVariant = Color(0xFF30353F),
        tertiary = Color(0xFF83DDB0),
    )

@Composable
internal fun BexTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = BexColors, content = content)
}
