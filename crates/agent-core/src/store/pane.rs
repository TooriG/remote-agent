//! A pane borrows its Host's Store; it never owns another transport or save loop.
use super::*;
use crate::presentation::view::Presentation;

pub struct Pane {
    store: Arc<Store>,
    id: String,
    presentations: watch::Receiver<Arc<Presentation>>,
    stop: CancellationToken,
    selection_quoted: tokio::sync::Mutex<bool>,
}

impl Store {
    pub fn primary_pane(self: &Arc<Self>) -> Arc<Pane> {
        Arc::new(Pane {
            store: self.clone(),
            id: String::new(),
            presentations: self.subscribe_presentation(),
            stop: self.stop.child_token(),
            selection_quoted: Default::default(),
        })
    }

    pub fn open_pane(self: &Arc<Self>, cwd: String) -> Result<Arc<Pane>, PeerError> {
        let id = self.create_pane(cwd)?;
        let stop = self.stop.child_token();
        let presentations = self.subscribe_pane(id.clone(), stop.clone());
        Ok(Arc::new(Pane {
            store: self.clone(),
            id,
            presentations,
            stop,
            selection_quoted: Default::default(),
        }))
    }
}

impl Pane {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn snapshot(&self) -> Result<Arc<Snapshot>, PeerError> {
        if self.stop.is_cancelled() {
            return Err(PeerError::ConnectionClosed("pane is closed".into()));
        }
        self.store
            .snapshot()
            .for_pane(&self.id)
            .map(Arc::new)
            .ok_or_else(|| PeerError::ConnectionClosed("pane is closed".into()))
    }

    pub fn presentation(&self) -> Result<Arc<Presentation>, PeerError> {
        Ok(Presentation::current(
            self.snapshot()?,
            &self.presentations.borrow(),
        ))
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Presentation>> {
        self.presentations.clone()
    }

    pub fn dispatch(
        &self,
        intent: Intent,
    ) -> impl Future<Output = Result<Outcome, PeerError>> + Send + use<> {
        if self.stop.is_cancelled() {
            futures_util::future::Either::Left(std::future::ready(Err(
                PeerError::ConnectionClosed("pane is closed".into()),
            )))
        } else {
            futures_util::future::Either::Right(self.store.dispatch_pane(&self.id, intent))
        }
    }

    pub fn prepare_dictation(&self) -> crate::client::DictationPreparation {
        self.store.prepare_dictation()
    }

    /// Preparation belongs to the pane, including when its native view unmounts.
    /// Serializing retries prevents duplicate sessions and preserves an edited quote.
    pub fn prepare_side_chat(
        self: &Arc<Self>,
        provider: Option<agent_protocol::session::ProviderKind>,
        selection: String,
    ) -> impl Future<Output = Result<Arc<Presentation>, PeerError>> + Send + use<> {
        let pane = self.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = pane.stop.cancelled() => Err(PeerError::ConnectionClosed("pane is closed".into())),
                result = pane.initialize_side_chat(provider, selection) => result,
            }
        });
        async move {
            task.await
                .map_err(|reason| PeerError::ConnectionClosed(reason.to_string()))?
        }
    }

    async fn initialize_side_chat(
        &self,
        provider: Option<agent_protocol::session::ProviderKind>,
        selection: String,
    ) -> Result<Arc<Presentation>, PeerError> {
        let mut quoted = self.selection_quoted.lock().await;
        let snapshot = self.snapshot()?;
        if snapshot.pane.navigation.thread_id.is_none() {
            let provider = provider
                .or_else(|| {
                    snapshot.model_provider_for_draft(snapshot.pane.navigation.draft_key.clone())
                })
                .ok_or_else(|| {
                    PeerError::InvalidMessage("利用可能なエージェントを選択してください。".into())
                })?;
            self.dispatch(Intent::CreateSession(op::CreateSession {
                provider,
                cwd: Some(snapshot.pane.navigation.cwd.clone()),
                model: None,
                initial_input: None,
            }))
            .await?;
        }
        if !*quoted {
            if !selection.is_empty() {
                self.dispatch(Intent::SetDraftText {
                    thread_id: self.snapshot()?.pane.navigation.draft_key.clone(),
                    text: crate::presentation::markdown::selection_quote(selection),
                })
                .await?;
            }
            *quoted = true;
        }
        self.presentation()
    }

    pub fn close(&self) {
        self.stop.cancel();
        self.store.close_pane(&self.id);
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        self.close();
    }
}
