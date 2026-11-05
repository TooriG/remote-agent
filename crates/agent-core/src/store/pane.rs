//! A pane borrows its Host's Store; it never owns another transport or save loop.
use super::*;
use crate::presentation::view::Presentation;

pub struct Pane {
    store: Arc<Store>,
    id: String,
    presentations: watch::Receiver<Arc<Presentation>>,
    stop: CancellationToken,
}

impl Store {
    pub fn primary_pane(self: &Arc<Self>) -> Arc<Pane> {
        Arc::new(Pane {
            store: self.clone(),
            id: String::new(),
            presentations: self.subscribe_presentation(),
            stop: self.stop.child_token(),
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
