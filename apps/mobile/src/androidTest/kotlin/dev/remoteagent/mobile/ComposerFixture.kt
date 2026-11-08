package dev.remoteagent.mobile

import androidx.compose.runtime.Composable
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Presentation
import dev.remoteagent.core.Snapshot

@Composable
internal fun ComposerFixture(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    onSend: () -> Unit,
    attach: @Composable ((Boolean) -> Unit) -> Unit,
) {
    val presentation = Presentation.fromSnapshot(snapshot, "")
    ThreadComposer(
        presentation.composer(),
        perform,
        onSend,
        settings = { selected, dismiss ->
            ModelPickerDialog(
                presentation.settings(),
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
    VoiceInputButton(Presentation.fromSnapshot(snapshot, "").composer(), perform, enabled)
}
