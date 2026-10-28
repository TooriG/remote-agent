//! One mobile Host session owns restoration, queued intents and ordered saves.
//! Platforms supply private storage; no UI task decides when state is durable.
use super::native::{AgentError, AgentStore, Connection, Receipt, error};
use crate::{
    diagnostics::ConnectionPhase,
    state::{Intent, Snapshot},
    store::Outcome,
};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Methods run on blocking workers, never on the platform's UI thread.
#[uniffi::export(rust, foreign)]
pub trait ClientStorage: Send + Sync {
    fn load(&self, new_pairing: bool) -> Result<Vec<u8>, AgentError>;
    fn save(&self, local_state: Vec<u8>, model_preferences: Vec<u8>) -> Result<(), AgentError>;
    fn identity(&self) -> Result<Vec<u8>, AgentError>;
    fn diagnostics_directory(&self) -> Result<Option<String>, AgentError>;
}

type Pending = (Intent, oneshot::Sender<Result<Outcome, AgentError>>);
type Completion<T> = Shared<BoxFuture<'static, Result<T, AgentError>>>;
enum Phase {
    Loading(Vec<Pending>),
    Ready {
        store: Arc<AgentStore>,
        runtime: tokio::runtime::Handle,
        observing: bool,
    },
    Failed(AgentError),
}

#[derive(uniffi::Object)]
pub struct ClientSession {
    persistence: Arc<Persistence>,
    previous: Mutex<Option<Arc<ClientSession>>>,
    closing: Mutex<Option<Completion<()>>>,
    recent_chats: bool,
    client_build: u64,
    phase: Mutex<Phase>,
    initialization: tokio::sync::Mutex<()>,
    operations: Mutex<Vec<Completion<Outcome>>>,
    stop: CancellationToken,
    empty: Arc<Snapshot>,
}

#[uniffi::export(async_runtime = "tokio")]
impl ClientSession {
    /// Synchronous creation lets intents queue before any native task starts.
    #[uniffi::constructor]
    pub fn open(
        storage: Arc<dyn ClientStorage>,
        previous: Option<Arc<ClientSession>>,
        recent_chats: bool,
        client_build: u64,
    ) -> Arc<Self> {
        if let Some(previous) = &previous {
            previous.retire();
        }
        Arc::new(Self {
            persistence: Arc::new(Persistence {
                storage,
                queue: Default::default(),
                error: Arc::default(),
            }),
            previous: Mutex::new(previous),
            closing: Default::default(),
            recent_chats,
            client_build,
            phase: Mutex::new(Phase::Loading(Vec::new())),
            initialization: Default::default(),
            operations: Default::default(),
            stop: CancellationToken::new(),
            empty: Arc::default(),
        })
    }

    /// Pairing stays provisional until the app commits its platform profile.
    #[uniffi::constructor]
    pub async fn pair(
        storage: Arc<dyn ClientStorage>,
        previous: Option<Arc<ClientSession>>,
        ticket: String,
        invitation: String,
        recent_chats: bool,
        client_build: u64,
    ) -> Result<Arc<Self>, AgentError> {
        // Device-wide model preferences must include edits still waiting for I/O.
        // Keep the current Host usable if pairing subsequently fails.
        if let Some(previous) = previous {
            previous.flush().await?;
        }
        let session = Self::open(storage, None, recent_chats, client_build);
        let store = session.restore(true).await?;
        store
            .reconnect(session.connection(ticket, Some(invitation)).await?)
            .await?;
        session.install(store, false)?;
        Ok(session)
    }

    pub fn store(&self) -> Option<Arc<AgentStore>> {
        match &*self.phase.lock().unwrap() {
            Phase::Ready { store, .. } => Some(store.clone()),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.store()
            .map_or_else(|| self.empty.clone(), |store| store.snapshot())
    }

    pub fn persistence_error(&self) -> Option<String> {
        self.persistence.error.lock().unwrap().clone()
    }

    pub async fn ready(self: Arc<Self>) -> Result<Arc<AgentStore>, AgentError> {
        let _initializing = self.initialization.lock().await;
        if self.stop.is_cancelled() {
            return Err(error("session closed"));
        }
        let previous = self.previous.lock().unwrap().clone();
        if let Some(previous) = previous {
            if let Err(reason) = previous.shutdown().await {
                self.fail(reason.clone());
                return Err(reason);
            }
            self.previous.lock().unwrap().take();
        }
        let status = match &mut *self.phase.lock().unwrap() {
            Phase::Loading(_) => None,
            Phase::Ready {
                store, observing, ..
            } => {
                let start = !*observing;
                *observing = true;
                Some(Ok((store.clone(), start)))
            }
            Phase::Failed(reason) => Some(Err(reason.clone())),
        };
        let (store, observe) = match status {
            Some(result) => result?,
            None => {
                let result = tokio::select! {
                    biased;
                    _ = self.stop.cancelled() => Err(error("session closed")),
                    result = self.restore(false) => result,
                };
                match result {
                    Ok(store) => {
                        if let Err(reason) = self.install(store.clone(), true) {
                            let _ = store.shutdown().await;
                            return Err(reason);
                        }
                        (store, true)
                    }
                    Err(reason) => {
                        self.fail(reason.clone());
                        return Err(reason);
                    }
                }
            }
        };
        if observe {
            self.observe(store.clone());
        }
        Ok(store)
    }

    /// The queue and ready transition share one lock, preserving input order.
    pub fn dispatch(&self, intent: Intent) -> Result<Arc<Receipt>, AgentError> {
        let (complete, result) = oneshot::channel();
        let mut phase = self.phase.lock().unwrap();
        match &mut *phase {
            Phase::Loading(pending) => pending.push((intent, complete)),
            Phase::Ready { store, runtime, .. } => {
                Self::dispatch_to(store, runtime, intent, complete)
            }
            Phase::Failed(reason) => return Err(reason.clone()),
        }
        let completion = async move { result.await.map_err(error)? }.boxed().shared();
        let mut operations = self.operations.lock().unwrap();
        operations.retain(|operation| operation.peek().is_none());
        operations.push(completion.clone());
        Ok(Arc::new(Receipt {
            result: Mutex::new(Some(Box::pin(completion))),
        }))
    }

    pub async fn resume(self: Arc<Self>, ticket: String) -> Result<bool, AgentError> {
        let store = self.clone().ready().await?;
        let reused = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(error("session closed")),
            result = async { store.resume(self.connection(ticket, None).await?).await } => result?,
        };
        if store.snapshot().host_name.is_none() {
            self.dispatch(Intent::LoadHostName(
                crate::state::operations::LoadHostName {},
            ))?;
        }
        Ok(reused)
    }

    /// Wait for all intents accepted before this flush, then capture under the
    /// write lock so an older autosave cannot replace the flushed snapshot.
    pub async fn flush(self: Arc<Self>) -> Result<(), AgentError> {
        let store = self.clone().ready().await?;
        let operations = self.operations.lock().unwrap().clone();
        for operation in operations {
            let _ = operation.await;
        }
        self.operations
            .lock()
            .unwrap()
            .retain(|operation| operation.peek().is_none());
        self.save(store)?.await
    }

    /// Start closing synchronously. Native cancellation cannot discard the final save.
    pub fn retire(&self) {
        let mut closing = self.closing.lock().unwrap();
        if closing.is_some() {
            return;
        }
        self.stop.cancel();
        if let Some(previous) = self.previous.lock().unwrap().as_ref() {
            previous.retire();
        }
        if let Phase::Ready {
            store,
            runtime,
            observing,
        } = self.fail(error("session closed"))
        {
            let persistence = self.persistence.clone();
            let (complete, result) = oneshot::channel();
            runtime.spawn(async move {
                let closed = store.shutdown().await;
                let saved = if observing {
                    match persistence.enqueue(store) {
                        Ok(saved) => saved.await,
                        Err(reason) => Err(reason),
                    }
                } else {
                    Ok(())
                };
                let _ = complete.send(closed.and(saved));
            });
            *closing = Some(async move { result.await.map_err(error)? }.boxed().shared());
        } else {
            *closing = Some(async { Ok(()) }.boxed().shared());
        }
    }

    pub async fn shutdown(self: Arc<Self>) -> Result<(), AgentError> {
        self.retire();
        let previous = self.previous.lock().unwrap().clone();
        if let Some(previous) = previous {
            Box::pin(previous.shutdown()).await?;
        }
        let closing = self.closing.lock().unwrap().as_ref().unwrap().clone();
        closing.await
    }
}

impl ClientSession {
    async fn restore(&self, new_pairing: bool) -> Result<Arc<AgentStore>, AgentError> {
        let started = std::time::Instant::now();
        let storage = self.persistence.storage.clone();
        let (bytes, directory) = tokio::task::spawn_blocking(move || {
            Ok::<_, AgentError>((storage.load(new_pairing)?, storage.diagnostics_directory()?))
        })
        .await
        .map_err(error)??;
        let read = started.elapsed().as_micros() as u64;
        let store = AgentStore::offline(bytes, directory).await?;
        store.record_connection_event(ConnectionPhase::SnapshotRead, read);
        store.record_connection_event(
            ConnectionPhase::AppPreparation,
            started.elapsed().as_micros() as u64,
        );
        store.record_connection_event(ConnectionPhase::ClientBuild, self.client_build);
        Ok(store)
    }

    async fn connection(
        &self,
        ticket: String,
        invitation: Option<String>,
    ) -> Result<Connection, AgentError> {
        let started = std::time::Instant::now();
        let storage = self.persistence.storage.clone();
        let identity = tokio::task::spawn_blocking(move || storage.identity())
            .await
            .map_err(error)??;
        if let Some(store) = self.store() {
            store.record_connection_event(
                ConnectionPhase::IdentityRead,
                started.elapsed().as_micros() as u64,
            );
        }
        Ok(Connection {
            ticket,
            identity,
            invitation,
            use_relays: true,
        })
    }

    fn install(&self, store: Arc<AgentStore>, observing: bool) -> Result<(), AgentError> {
        let runtime = tokio::runtime::Handle::current();
        let mut phase = self.phase.lock().unwrap();
        let Phase::Loading(pending) = &mut *phase else {
            return Err(error("session closed"));
        };
        self.persistence.start();
        // Navigation comes before queued UI intents, just as on both platforms.
        let _ = store.dispatch(Intent::ShowThreadList);
        if self.recent_chats {
            let _ = store.dispatch(Intent::ShowRecentChats);
        }
        for (intent, complete) in pending.drain(..) {
            Self::dispatch_to(&store, &runtime, intent, complete);
        }
        *phase = Phase::Ready {
            store,
            runtime,
            observing,
        };
        Ok(())
    }

    fn dispatch_to(
        store: &AgentStore,
        runtime: &tokio::runtime::Handle,
        intent: Intent,
        complete: oneshot::Sender<Result<Outcome, AgentError>>,
    ) {
        let receipt = store.dispatch(intent);
        runtime.spawn(async move {
            let result = match receipt {
                Ok(receipt) => receipt.wait().await,
                Err(reason) => Err(reason),
            };
            let _ = complete.send(result);
        });
    }

    fn fail(&self, reason: AgentError) -> Phase {
        let previous = std::mem::replace(
            &mut *self.phase.lock().unwrap(),
            Phase::Failed(reason.clone()),
        );
        match previous {
            Phase::Loading(pending) => {
                for (_, complete) in pending {
                    let _ = complete.send(Err(reason.clone()));
                }
                Phase::Failed(reason)
            }
            other => other,
        }
    }

    fn observe(self: &Arc<Self>, store: Arc<AgentStore>) {
        let session = Arc::downgrade(self);
        let stop = self.stop.clone();
        tokio::spawn(async move {
            let mut previous = store.snapshot();
            let mut dirty = true;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return,
                    next = store.next_snapshot(previous.clone()) => match next { Ok(next) => Some(next), Err(_) => return },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(250)), if dirty => None,
                };
                if let Some(next) = next {
                    let immediate = !next.list_unchanged(previous.clone());
                    previous = next;
                    dirty = true;
                    if !immediate {
                        continue;
                    }
                }
                let Some(session) = session.upgrade() else {
                    return;
                };
                if let Ok(saved) = session.save(store.clone()) {
                    let _ = saved.await;
                }
                dirty = false;
            }
        });
    }
    fn save(&self, store: Arc<AgentStore>) -> Result<Completion<()>, AgentError> {
        let phase = self.phase.lock().unwrap();
        match &*phase {
            Phase::Ready { .. } => self.persistence.enqueue(store),
            _ => Err(error("session closed")),
        }
    }
}

struct Save {
    store: Arc<AgentStore>,
    complete: oneshot::Sender<Result<(), AgentError>>,
}
struct Persistence {
    storage: Arc<dyn ClientStorage>,
    queue: Mutex<Option<mpsc::UnboundedSender<Save>>>,
    error: Arc<Mutex<Option<String>>>,
}
impl Persistence {
    fn start(&self) {
        let mut queue = self.queue.lock().unwrap();
        if queue.is_some() {
            return;
        }
        let (send, mut receive) = mpsc::unbounded_channel::<Save>();
        *queue = Some(send);
        let storage = self.storage.clone();
        let failure = self.error.clone();
        // This worker owns each write through completion, even if its native
        // waiter is cancelled. Closing enqueues the final save after earlier ones.
        tokio::spawn(async move {
            while let Some(Save { store, complete }) = receive.recv().await {
                let storage = storage.clone();
                let snapshot = store.snapshot();
                let result = tokio::task::spawn_blocking(move || {
                    storage.save(
                        snapshot.serialize_local_state()?,
                        snapshot.serialize_model_preferences()?,
                    )
                })
                .await
                .map_err(error)
                .and_then(|result| result);
                *failure.lock().unwrap() = result.as_ref().err().map(ToString::to_string);
                let _ = complete.send(result);
            }
        });
    }

    fn enqueue(&self, store: Arc<AgentStore>) -> Result<Completion<()>, AgentError> {
        let (complete, result) = oneshot::channel();
        self.queue
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| error("session is not ready"))?
            .send(Save { store, complete })
            .map_err(error)?;
        Ok(async move { result.await.map_err(error)? }.boxed().shared())
    }
}

impl Drop for ClientSession {
    fn drop(&mut self) {
        self.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Condvar,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Gate {
        entered: Notify,
        released: Mutex<bool>,
        wake: Condvar,
    }
    impl Gate {
        fn block(&self) {
            self.entered.notify_one();
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.wake.wait(released).unwrap();
            }
        }
        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.wake.notify_all();
        }
    }

    #[derive(Default)]
    struct Storage {
        bytes: Mutex<Vec<u8>>,
        writes: Mutex<Vec<Vec<u8>>>,
        loads: AtomicUsize,
        read_gate: Mutex<Option<Arc<Gate>>>,
        write_gate: Mutex<Option<Arc<Gate>>>,
        fail_save: AtomicBool,
        saved: Notify,
    }
    impl ClientStorage for Storage {
        fn load(&self, new_pairing: bool) -> Result<Vec<u8>, AgentError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            let gate = self.read_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.block();
            }
            Ok(if new_pairing {
                Vec::new()
            } else {
                self.bytes.lock().unwrap().clone()
            })
        }
        fn save(&self, bytes: Vec<u8>, preferences: Vec<u8>) -> Result<(), AgentError> {
            let gate = self.write_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.block();
            }
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(error("disk unavailable"));
            }
            // Both platform payloads must use their production codecs.
            crate::persistence::decode(&bytes).unwrap();
            crate::persistence::apply_model_preferences(&[], &preferences).unwrap();
            *self.bytes.lock().unwrap() = bytes.clone();
            self.writes.lock().unwrap().push(bytes);
            self.saved.notify_one();
            Ok(())
        }
        fn identity(&self) -> Result<Vec<u8>, AgentError> {
            Ok(vec![1; 32])
        }
        fn diagnostics_directory(&self) -> Result<Option<String>, AgentError> {
            Ok(None)
        }
    }
    fn edit(text: &str) -> Intent {
        Intent::SetDraftText {
            thread_id: "draft".into(),
            text: text.into(),
        }
    }
    fn text(bytes: &[u8]) -> String {
        crate::persistence::decode(bytes).unwrap().drafts[&"draft".into()]
            .text
            .clone()
    }
    fn open(storage: Arc<Storage>) -> Arc<ClientSession> {
        ClientSession::open(storage, None, false, 42)
    }

    #[tokio::test]
    async fn pairing_reads_device_preferences_after_pending_saves_and_keeps_the_current_host_on_failure()
     {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let current_storage = Arc::new(Storage::default());
            let current = open(current_storage.clone());
            current.clone().ready().await.unwrap();
            current.dispatch(edit("unsaved current Host")).unwrap();
            let gate = Arc::new(Gate::default());
            let _release = scopeguard::guard(gate.clone(), |gate| gate.release());
            *current_storage.write_gate.lock().unwrap() = Some(gate.clone());
            let storage = Arc::new(Storage::default());
            let pairing = tokio::spawn(ClientSession::pair(
                storage.clone(),
                Some(current.clone()),
                "invalid ticket".into(),
                "invalid invitation".into(),
                false,
                0,
            ));
            gate.entered.notified().await;
            assert_eq!(storage.loads.load(Ordering::SeqCst), 0);
            gate.release();
            assert!(pairing.await.unwrap().is_err());
            assert_eq!(
                text(&current_storage.bytes.lock().unwrap()),
                "unsaved current Host"
            );
            current.dispatch(edit("still usable")).unwrap();
            current.shutdown().await.unwrap();
            assert_eq!(text(&current_storage.bytes.lock().unwrap()), "still usable");
        })
        .await
        .expect("pairing did not flush device preferences");
    }

    #[tokio::test]
    async fn adopting_a_provisional_pair_enables_autosave_while_discarding_it_keeps_saved_work() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            *storage.bytes.lock().unwrap() = b"previous pairing".to_vec();
            let provisional = open(storage.clone());
            provisional
                .install(AgentStore::offline(Vec::new(), None).await.unwrap(), false)
                .unwrap();
            provisional.shutdown().await.unwrap();
            assert_eq!(*storage.bytes.lock().unwrap(), b"previous pairing");
            assert!(storage.writes.lock().unwrap().is_empty());

            let adopted = open(storage.clone());
            adopted
                .install(AgentStore::offline(Vec::new(), None).await.unwrap(), false)
                .unwrap();
            adopted.clone().ready().await.unwrap();
            adopted.dispatch(edit("paired Host work")).unwrap();
            storage.saved.notified().await;
            assert_eq!(text(&storage.bytes.lock().unwrap()), "paired Host work");
            adopted.shutdown().await.unwrap();
        })
        .await
        .expect("pair adoption did not start persistence");
    }

    #[tokio::test]
    async fn restoration_applies_queued_intents_in_order_and_flushes_without_native_waiters() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            let original = crate::store::Store::offline(Snapshot::default());
            original.dispatch(edit("cached")).await.unwrap();
            *storage.bytes.lock().unwrap() =
                crate::persistence::encode(&original.snapshot()).unwrap();
            original.close().await.unwrap();
            let session = open(storage.clone());
            let first = session.dispatch(edit("first edit")).unwrap();
            let second = session.dispatch(edit("last edit")).unwrap();
            assert!(session.store().is_none());
            session.clone().ready().await.unwrap();
            assert!(session.store().is_some());
            assert_eq!(session.snapshot().drafts[&"draft".into()].text, "last edit");
            session.clone().flush().await.unwrap();
            assert_eq!(text(&storage.bytes.lock().unwrap()), "last edit");
            first.wait().await.unwrap();
            second.wait().await.unwrap();
            let store = session.clone().ready().await.unwrap();
            assert!(Arc::ptr_eq(&store, &session.clone().ready().await.unwrap()));
            assert_eq!(storage.loads.load(Ordering::SeqCst), 1);
            // Once ready, dispatch remains synchronous even on a native UI thread.
            session.dispatch(edit("synchronous edit")).unwrap();
            assert_eq!(
                session.snapshot().drafts[&"draft".into()].text,
                "synchronous edit"
            );
            session.shutdown().await.unwrap();
            assert_eq!(text(&storage.bytes.lock().unwrap()), "synchronous edit");
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn damaged_state_fails_every_queued_receipt_without_overwriting_user_work() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            *storage.bytes.lock().unwrap() = b"damaged state".to_vec();
            let session = open(storage.clone());
            let receipt = session
                .dispatch(edit("must not replace saved work"))
                .unwrap();
            assert!(session.clone().ready().await.is_err());
            assert!(receipt.wait().await.is_err());
            assert!(session.dispatch(edit("rejected")).is_err());
            assert!(session.clone().flush().await.is_err());
            session.shutdown().await.unwrap();
            assert_eq!(*storage.bytes.lock().unwrap(), b"damaged state");
            assert!(storage.writes.lock().unwrap().is_empty());
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn closing_cancels_all_queued_receipts_without_loading() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            let session = open(storage.clone());
            let receipts: Vec<_> = (0..256)
                .map(|_| session.dispatch(edit("queued")).unwrap())
                .collect();
            session.retire();
            for receipt in receipts {
                assert!(receipt.wait().await.is_err());
            }
            assert!(session.clone().ready().await.is_err());
            assert!(session.dispatch(edit("closed")).is_err());
            session.shutdown().await.unwrap();
            assert_eq!(storage.loads.load(Ordering::SeqCst), 0);
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn closing_during_a_blocked_read_never_installs_the_obsolete_store() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            let gate = Arc::new(Gate::default());
            let _release = scopeguard::guard(gate.clone(), |gate| gate.release());
            *storage.read_gate.lock().unwrap() = Some(gate.clone());
            let session = open(storage.clone());
            let receipt = session.dispatch(edit("old Host")).unwrap();
            let loading = tokio::spawn(session.clone().ready());
            gate.entered.notified().await;
            session.retire();
            assert!(loading.await.unwrap().is_err());
            assert!(receipt.wait().await.is_err());
            gate.release();
            session.clone().shutdown().await.unwrap();
            assert!(session.store().is_none());
            assert!(storage.writes.lock().unwrap().is_empty());
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn cancelling_a_flush_cannot_release_its_write_lock_before_io_finishes() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            let session = open(storage.clone());
            session.clone().ready().await.unwrap();
            session.dispatch(edit("old edit")).unwrap();
            let gate = Arc::new(Gate::default());
            let _release = scopeguard::guard(gate.clone(), |gate| gate.release());
            *storage.write_gate.lock().unwrap() = Some(gate.clone());
            let writing = tokio::spawn(session.clone().flush());
            gate.entered.notified().await;
            writing.abort();
            assert!(writing.await.unwrap_err().is_cancelled());
            session.dispatch(edit("latest edit")).unwrap();
            let final_write = tokio::spawn(session.clone().flush());
            assert!(
                tokio::time::timeout(
                    std::time::Duration::from_millis(30),
                    storage.saved.notified()
                )
                .await
                .is_err()
            );
            gate.release();
            final_write.await.unwrap().unwrap();
            session.shutdown().await.unwrap();
            assert_eq!(text(&storage.bytes.lock().unwrap()), "latest edit");
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn switching_hosts_waits_for_the_final_save_even_if_the_old_native_waiter_disappears() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let old_storage = Arc::new(Storage::default());
            let old = open(old_storage.clone());
            old.clone().ready().await.unwrap();
            old.dispatch(edit("preserve old Host")).unwrap();
            let gate = Arc::new(Gate::default());
            let _release = scopeguard::guard(gate.clone(), |gate| gate.release());
            *old_storage.write_gate.lock().unwrap() = Some(gate.clone());
            let next_storage = Arc::new(Storage::default());
            let next = ClientSession::open(next_storage.clone(), Some(old.clone()), true, 0);
            gate.entered.notified().await;
            let opening = tokio::spawn(next.clone().ready());
            opening.abort();
            let _ = opening.await;
            assert!(old.dispatch(edit("obsolete input")).is_err());
            assert_eq!(next_storage.loads.load(Ordering::SeqCst), 0);
            drop(old);
            let receipt = next.dispatch(edit("new Host")).unwrap();
            gate.release();
            next.clone().flush().await.unwrap();
            receipt.wait().await.unwrap();
            assert!(next.snapshot().recent_chats);
            assert_eq!(
                text(&old_storage.bytes.lock().unwrap()),
                "preserve old Host"
            );
            assert_eq!(text(&next_storage.bytes.lock().unwrap()), "new Host");
            next.shutdown().await.unwrap();
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn saving_recovers_after_storage_returns_and_dropping_a_session_flushes_its_last_edit() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            let session = open(storage.clone());
            session.clone().ready().await.unwrap();
            session.dispatch(edit("unsaved work")).unwrap();
            storage.fail_save.store(true, Ordering::SeqCst);
            assert!(session.clone().flush().await.is_err());
            assert_eq!(
                session.persistence_error().as_deref(),
                Some("disk unavailable")
            );
            storage.fail_save.store(false, Ordering::SeqCst);
            session.clone().flush().await.unwrap();
            assert!(session.persistence_error().is_none());
            assert_eq!(text(&storage.bytes.lock().unwrap()), "unsaved work");
            // Drain the notification from the explicit flush.
            storage.saved.notified().await;
            session.dispatch(edit("last edit before dropping")).unwrap();
            drop(session);
            tokio::time::timeout(std::time::Duration::from_secs(2), storage.saved.notified())
                .await
                .unwrap();
            assert_eq!(
                text(&storage.bytes.lock().unwrap()),
                "last edit before dropping"
            );
        })
        .await
        .expect("session lifecycle stalled");
    }

    #[tokio::test]
    async fn a_failed_pairing_does_not_replace_a_previously_saved_host_snapshot() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let storage = Arc::new(Storage::default());
            *storage.bytes.lock().unwrap() = b"saved work must remain".to_vec();
            assert!(
                ClientSession::pair(
                    storage.clone(),
                    None,
                    "invalid ticket".into(),
                    "invalid invitation".into(),
                    false,
                    0
                )
                .await
                .is_err()
            );
            assert_eq!(*storage.bytes.lock().unwrap(), b"saved work must remain");
            assert!(storage.writes.lock().unwrap().is_empty());
        })
        .await
        .expect("session lifecycle stalled");
    }
}
