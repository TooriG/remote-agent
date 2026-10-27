package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private const val THEME_PRIMARY = 0xFFB5A0F5
private const val THEME_ON_PRIMARY = 0xFF1D1532
private const val THEME_PRIMARY_CONTAINER = 0xFF352B4D
private const val THEME_ON_PRIMARY_CONTAINER = 0xFFE9DFFF
private const val THEME_BACKGROUND = 0xFF111316
private const val THEME_ON_BACKGROUND = 0xFFF0F1F3
private const val THEME_SURFACE = 0xFF111316
private const val THEME_ON_SURFACE = 0xFFF0F1F3
private const val THEME_SURFACE_VARIANT = 0xFF20242B
private const val THEME_ON_SURFACE_VARIANT = 0xFF989DA8
private const val THEME_OUTLINE = 0xFF565D69
private const val THEME_OUTLINE_VARIANT = 0xFF30353F
private const val THEME_TERTIARY = 0xFF83DDB0

private val AppColors =
    darkColorScheme(
        primary = Color(THEME_PRIMARY),
        onPrimary = Color(THEME_ON_PRIMARY),
        primaryContainer = Color(THEME_PRIMARY_CONTAINER),
        onPrimaryContainer = Color(THEME_ON_PRIMARY_CONTAINER),
        background = Color(THEME_BACKGROUND),
        onBackground = Color(THEME_ON_BACKGROUND),
        surface = Color(THEME_SURFACE),
        onSurface = Color(THEME_ON_SURFACE),
        surfaceVariant = Color(THEME_SURFACE_VARIANT),
        onSurfaceVariant = Color(THEME_ON_SURFACE_VARIANT),
        outline = Color(THEME_OUTLINE),
        outlineVariant = Color(THEME_OUTLINE_VARIANT),
        tertiary = Color(THEME_TERTIARY),
    )

@Composable
internal fun AppTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = AppColors, content = content)
}
