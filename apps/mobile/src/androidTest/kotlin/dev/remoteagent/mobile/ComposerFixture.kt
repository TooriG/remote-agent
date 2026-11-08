package dev.remoteagent.mobile

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot

@Composable
internal fun ModelPickerFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    ModelPickerButton(dev.remoteagent.core.Presentation.fromSnapshot(snapshot, "").composer(), perform, modifier) {
        selected,
        dismiss ->
        ModelPickerDialog(
            dev.remoteagent.core.Presentation.fromSnapshot(snapshot, "").settings(),
            selected,
            perform,
            dismiss,
        )
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
        dev.remoteagent.core.Presentation.fromSnapshot(snapshot, "").composer(),
        perform,
        onSend,
        settings = { selected, dismiss ->
            ModelPickerDialog(
                dev.remoteagent.core.Presentation.fromSnapshot(snapshot, "").settings(),
                selected,
                perform,
                dismiss,
            )
        },
        attach = attach,
    )
}

@Composable
internal fun VoiceInputFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    enabled: Boolean,
) {
    VoiceInputButton(dev.remoteagent.core.Presentation.fromSnapshot(snapshot, "").composer(), perform, enabled)
}
