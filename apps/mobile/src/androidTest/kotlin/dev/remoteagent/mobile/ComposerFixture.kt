package dev.remoteagent.mobile

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import dev.remoteagent.core.ComposerPresentation
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot

@Composable
internal fun ModelPickerFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    ModelPickerButton(ComposerPresentation.fromSnapshot(snapshot), perform, modifier) { selected, dismiss ->
        ModelPickerDialog(snapshot, selected, perform, dismiss)
    }
}

@Composable
internal fun ComposerFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    onSend: () -> Unit,
    attach: @Composable ((Boolean) -> Unit) -> Unit,
) {
    ThreadComposer(
        ComposerPresentation.fromSnapshot(snapshot),
        perform,
        onSend,
        settings = { selected, dismiss -> ModelPickerDialog(snapshot, selected, perform, dismiss) },
        attach = attach,
    )
}

@Composable
internal fun VoiceInputFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    enabled: Boolean,
) {
    VoiceInputButton(ComposerPresentation.fromSnapshot(snapshot), perform, enabled)
}
