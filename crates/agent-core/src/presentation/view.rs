//! Coherent native presentation, projected independently of state transitions.
use super::{
    conversation::{RenderedConversation, project_conversation},
    list::ThreadList,
};
use crate::state::{Changed, Snapshot};
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

#[cfg_attr(feature = "bindings", derive(uniffi::Object))]
pub struct Presentation {
    pub snapshot: Arc<Snapshot>,
    pub list: Option<Arc<ThreadList>>,
    pub conversation: Option<Arc<RenderedConversation>>,
    pub composer: Arc<super::composer::ComposerPresentation>,
    pub context: Arc<super::context::ConversationContext>,
    pub agents: Arc<Vec<super::agents::AgentSummary>>,
    pub settings: Arc<super::settings::SettingsPresentation>,
}

#[derive(Clone, Copy)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct PresentationChanges {
    pub changed: bool,
    pub groups: Changed,
    pub list: bool,
    pub conversation: bool,
    pub composer: bool,
    pub composer_controls: bool,
    pub context: bool,
    pub agents: bool,
    pub settings: bool,
    pub reconnected: bool,
}

impl Presentation {
    pub(crate) fn pending(snapshot: Arc<Snapshot>) -> Arc<Self> {
        let composer = super::composer::ComposerPresentation::project(&snapshot, None);
        let context = super::context::ConversationContext::project(&snapshot, None);
        let settings = super::settings::SettingsPresentation::project(
            &snapshot,
            composer.controls.provider,
            None,
        );
        Arc::new(Self {
            snapshot,
            list: None,
            conversation: None,
            composer,
            context,
            agents: super::agents::empty(),
            settings,
        })
    }

    pub fn project(snapshot: Arc<Snapshot>, previous: Option<&Self>) -> Arc<Self> {
        let list = if !snapshot.pane_id.is_empty() {
            None
        } else if let Some(previous) = previous
            && snapshot
                .list_source()
                .equivalent(&previous.snapshot.list_source())
        {
            previous.list.clone()
        } else {
            let source = snapshot.list_source();
            if snapshot.lists.recent_chats {
                source.recent_thread_list()
            } else {
                source.thread_list()
            }
            .map(Arc::new)
        };
        let source = snapshot.conversation_thread().map(|source| {
            // Pending input before session creation has no native Thread. Retain
            // that placeholder across unrelated publications as well.
            previous
                .and_then(|previous| previous.conversation.as_ref())
                .filter(|previous| source.id.is_none() && previous.source.id.is_none())
                .map_or(source, |previous| previous.source.clone())
        });
        let conversation = source.map(|source| {
            project_conversation(
                snapshot.catalog.ui_language,
                &snapshot.pane.navigation.draft_key,
                &snapshot.sessions.pending_submissions,
                source,
                &previous.and_then(|previous| previous.conversation.clone()),
            )
        });
        let composer = super::composer::ComposerPresentation::project(
            &snapshot,
            previous.map(|previous| &previous.composer),
        );
        let context = super::context::ConversationContext::project(
            &snapshot,
            previous.map(|previous| &previous.context),
        );
        let agents = super::agents::project(&snapshot, previous);
        let settings = super::settings::SettingsPresentation::project(
            &snapshot,
            composer.controls.provider,
            previous,
        );
        Arc::new(Self {
            snapshot,
            list,
            conversation,
            composer,
            context,
            agents,
            settings,
        })
    }

    fn same_context(&self, latest: &Snapshot) -> bool {
        self.snapshot.catalog.ui_language == latest.catalog.ui_language
            && self.snapshot.pane.navigation.draft_key == latest.pane.navigation.draft_key
            && self.snapshot.pane.navigation.thread_id == latest.pane.navigation.thread_id
            && self.snapshot.connection.epoch == latest.connection.epoch
            && self.snapshot.pane.epoch == latest.pane.epoch
            && self.snapshot.connection.storage_scope == latest.connection.storage_scope
    }

    /// State acknowledgments remain immediate while native content catches up.
    /// Keep an ordered stream prefix only within the same selected conversation.
    pub(crate) fn current(snapshot: Arc<Snapshot>, content: &Arc<Self>) -> Arc<Self> {
        if Arc::ptr_eq(&snapshot, &content.snapshot) {
            return content.clone();
        }
        let same_host =
            snapshot.connection.storage_scope == content.snapshot.connection.storage_scope;
        let same_conversation = same_host
            && snapshot.pane.navigation.draft_key == content.snapshot.pane.navigation.draft_key
            && snapshot.pane.navigation.thread_id == content.snapshot.pane.navigation.thread_id;
        let composer =
            super::composer::ComposerPresentation::project(&snapshot, Some(&content.composer));
        let settings = super::settings::SettingsPresentation::project(
            &snapshot,
            composer.controls.provider,
            Some(content),
        );
        Arc::new(Self {
            agents: if same_conversation
                && snapshot.pane.observed_agents == content.snapshot.pane.observed_agents
                && snapshot.pane.observed_agents.is_some()
            {
                content.agents.clone()
            } else {
                super::agents::empty()
            },
            composer,
            settings,
            context: super::context::ConversationContext::project(
                &snapshot,
                Some(&content.context),
            ),
            snapshot,
            list: same_host.then(|| content.list.clone()).flatten(),
            conversation: same_conversation
                .then(|| content.conversation.clone())
                .flatten(),
        })
    }
}

#[cfg_attr(feature = "bindings", uniffi::export)]
impl Presentation {
    #[cfg_attr(feature = "bindings", uniffi::constructor)]
    pub fn empty(language: crate::localization::UiLanguage) -> Arc<Self> {
        let mut snapshot = Snapshot::default();
        snapshot.catalog.ui_language = language;
        Self::pending(Arc::new(snapshot))
    }
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.clone()
    }
    pub fn conversation(&self) -> Option<Arc<RenderedConversation>> {
        self.conversation.clone()
    }
    pub fn composer(&self) -> Arc<super::composer::ComposerPresentation> {
        self.composer.clone()
    }
    pub fn conversation_context(&self) -> Arc<super::context::ConversationContext> {
        self.context.clone()
    }
    pub fn thread_list(&self) -> Option<ThreadList> {
        self.list.as_deref().cloned()
    }
    pub fn agents(&self) -> Vec<super::agents::AgentSummary> {
        self.agents.as_ref().clone()
    }
    pub fn settings(&self) -> Arc<super::settings::SettingsPresentation> {
        self.settings.clone()
    }
    pub fn changes(&self, previous: Arc<Self>) -> PresentationChanges {
        use crate::state::Same;
        let list = !self.list.same(&previous.list);
        let conversation = !self.conversation.same(&previous.conversation);
        let composer = !self.composer.equivalent(&previous.composer);
        let context = self.context.as_ref() != previous.context.as_ref();
        let agents = !self.agents.same(&previous.agents);
        let settings = !self.settings.equivalent(&previous.settings);
        let groups = self.snapshot.changes_from(&previous.snapshot);
        PresentationChanges {
            changed: groups.any
                || list
                || conversation
                || composer
                || context
                || agents
                || settings,
            groups,
            list,
            conversation,
            composer,
            composer_controls: self.composer.controls != previous.composer.controls,
            context,
            agents,
            settings,
            reconnected: self.snapshot.connection.connected
                && self.snapshot.connection.connection_generation
                    != previous.snapshot.connection.connection_generation,
        }
    }
}

/// One ordered projector per Store. Watch coalesces queued snapshots, so a long
/// history cannot queue one expensive job per token or block dispatch/recovery.
/// A stream may publish an earlier coherent prefix while the next is queued;
/// results for a replaced navigation or connection are always discarded.
pub(crate) fn observe(
    mut snapshots: watch::Receiver<Arc<Snapshot>>,
    pane: String,
    stop: CancellationToken,
    project: impl Fn(Arc<Snapshot>, Option<&Presentation>) -> Arc<Presentation> + Send + Sync + 'static,
) -> watch::Receiver<Arc<Presentation>> {
    let initial = Presentation::pending(Arc::new(
        snapshots.borrow().for_pane(&pane).unwrap_or_default(),
    ));
    let (publish, presentations) = watch::channel(initial);
    let project = Arc::new(project);
    tokio::spawn(async move {
        let mut previous = None;
        loop {
            let Some(snapshot) = snapshots.borrow_and_update().for_pane(&pane).map(Arc::new) else {
                return;
            };
            publish_current(snapshot.clone(), &publish);
            let held = previous.clone();
            let project = project.clone();
            let mut projecting =
                tokio::task::spawn_blocking(move || project(snapshot, held.as_deref()));
            let mut catch_up = false;
            let next = loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return,
                    result = &mut projecting => match result { Ok(next) => break next, Err(_) => return },
                    result = snapshots.changed() => {
                        if result.is_err() { return; }
                        let Some(latest) = snapshots.borrow_and_update().for_pane(&pane).map(Arc::new) else { return; };
                        // Native input and navigation acknowledge immediately. Only
                        // the coherent transcript prefix waits for the worker.
                        publish_current(latest, &publish);
                        catch_up = true;
                    }
                }
            };
            let Some(latest) = snapshots.borrow().for_pane(&pane).map(Arc::new) else {
                return;
            };
            if next.same_context(&latest) {
                publish.send_replace(Presentation::current(latest, &next));
                previous = Some(next);
            }
            if catch_up || snapshots.has_changed().unwrap_or(false) {
                continue;
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                result = snapshots.changed() => if result.is_err() { return; },
            }
        }
    });
    presentations
}

fn publish_current(snapshot: Arc<Snapshot>, publish: &watch::Sender<Arc<Presentation>>) {
    let previous = publish.borrow().clone();
    let current = Presentation::current(snapshot, &previous);
    if current.changes(previous).changed {
        publish.send_replace(current);
    }
}
