package dev.remoteagent.mobile

import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.ConnectionSelection

@Composable
internal fun RowScope.ConnectionListTitle(
    profiles: List<HostProfile>,
    selection: ConnectionSelection,
    connected: Boolean,
    select: (String?) -> Unit,
    openSettings: () -> Unit,
) {
    Image(
        painterResource(R.drawable.agent_icon),
        "Bex",
        Modifier.padding(end = 8.dp).size(26.dp).clip(RoundedCornerShape(6.dp)),
    )
    Box(Modifier.weight(1f)) { if (profiles.isNotEmpty()) ConnectionMenu(profiles, selection, connected, select) }
    IconButton(onClick = openSettings) {
        Icon(painterResource(R.drawable.ic_globe), LocalUiLanguage.current.text("設定"))
    }
}

@Composable
private fun ConnectionMenu(
    profiles: List<HostProfile>,
    selection: ConnectionSelection,
    connected: Boolean,
    select: (String?) -> Unit,
) {
    val language = LocalUiLanguage.current
    var expanded by remember { mutableStateOf(false) }
    val name =
        if (selection.all) language.text("すべての接続先")
        else profiles.firstOrNull { it.id == selection.hostId }?.name.orEmpty()
    Box {
        Row(
            Modifier.heightIn(min = 48.dp).clickable(onClickLabel = language.text("接続先を切り替える")) { expanded = true },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                painterResource(R.drawable.ic_circle),
                language.text(if (connected) "接続中" else "未接続"),
                Modifier.size(6.dp),
                tint =
                    if (connected) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Icon(painterResource(R.drawable.ic_computer), null, Modifier.size(16.dp))
            Text(
                name,
                Modifier.weight(1f, fill = false),
                style = MaterialTheme.typography.labelLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Icon(painterResource(R.drawable.ic_chevron), null, Modifier.size(14.dp).rotate(CHEVRON_DOWN_ROTATION))
        }
        DropdownMenu(expanded, onDismissRequest = { expanded = false }) {
            DropdownMenuItem(
                text = { Text(language.text("すべての接続先")) },
                onClick = {
                    expanded = false
                    select(null)
                },
                trailingIcon = { if (selection.all) Text("✓") },
            )
            profiles.forEach { profile ->
                DropdownMenuItem(
                    text = { Text(profile.name) },
                    onClick = {
                        expanded = false
                        select(profile.id)
                    },
                    trailingIcon = { if (!selection.all && selection.hostId == profile.id) Text("✓") },
                )
            }
        }
    }
}

private const val CHEVRON_DOWN_ROTATION = 90f

@Composable
internal fun EmptyConnections(connect: () -> Unit, modifier: Modifier = Modifier) {
    val language = LocalUiLanguage.current
    Column(
        modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(painterResource(R.drawable.ic_computer), null, Modifier.size(48.dp))
        Text(language.text("作業する環境を接続"), Modifier.padding(top = 16.dp), style = MaterialTheme.typography.titleLarge)
        Text(language.text("接続すると、プロジェクトとタスクがここに表示されます"), Modifier.padding(vertical = 16.dp))
        Button(onClick = connect) { Text(language.text("環境に接続")) }
    }
}
