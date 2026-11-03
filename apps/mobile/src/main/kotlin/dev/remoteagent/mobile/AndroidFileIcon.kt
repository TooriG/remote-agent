package dev.remoteagent.mobile

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import dev.remoteagent.core.mobileFileIcon

/** Untinted upstream mobile Pierre glyphs; folder/navigation symbols use native vectors. */
@Composable
internal fun FileEntryIcon(path: String, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val token = mobileFileIcon(path)
    val bitmap =
        remember(token) {
            context.assets.open("pierre_$token.png").use {
                checkNotNull(BitmapFactory.decodeStream(it)).asImageBitmap()
            }
        }
    Image(bitmap, contentDescription = null, modifier = modifier)
}
