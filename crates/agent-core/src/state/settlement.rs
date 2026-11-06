//! Section pages and the latest observed organization. Decisions outlive rows so stale reads cannot resurrect them.
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettlementState {
    pub pages: BTreeMap<Option<String>, Arc<ThreadList>>,
    pub expanded: BTreeSet<Option<String>>,
    pub updates:
        BTreeMap<crate::session::SessionRef, agent_protocol::operations::SessionSettlement>,
}

impl Snapshot {
    pub(crate) fn reconcile_settlements(&mut self, rows: &mut [Thread]) {
        for row in rows {
            let Some(id) = &row.id else {
                continue;
            };
            let previous = self.lists.settlement.updates.get(id);
            if let Some(update) = previous
                && row.list_revision < update.revision
            {
                row.settled = update.settled;
            } else if row.list_revision > 0
                && previous.is_none_or(|old| {
                    old.revision != row.list_revision || old.settled != row.settled
                })
            {
                Arc::make_mut(&mut self.lists.settlement).updates.insert(
                    id.clone(),
                    agent_protocol::operations::SessionSettlement {
                        session: id.clone(),
                        settled: row.settled,
                        revision: row.list_revision,
                    },
                );
            }
        }
    }

    pub(crate) fn accept_settlement(
        &mut self,
        change: agent_protocol::operations::SessionSettlement,
    ) -> Vec<Effect> {
        if self
            .lists
            .settlement
            .updates
            .get(&change.session)
            .is_some_and(|old| old.revision >= change.revision)
        {
            return Vec::new();
        }
        let id = change.session.clone();
        let settled = change.settled;
        let revision = change.revision;
        Arc::make_mut(&mut self.lists.settlement)
            .updates
            .insert(id.clone(), change);
        if let Some(thread) = shared_mut(&mut self.sessions.conversations, &id)
            && thread.list_revision <= revision
        {
            thread.settled = settled;
            thread.list_revision = revision;
        }
        for page in self
            .lists
            .threads
            .iter_mut()
            .chain(Arc::make_mut(&mut self.lists.project_threads).values_mut())
        {
            update_page(page, &id, settled, revision);
        }
        for page in Arc::make_mut(&mut self.lists.settlement).pages.values_mut() {
            update_page(page, &id, settled, revision);
        }
        self.refresh_settlement_lists()
    }

    pub(crate) fn refresh_settlement_lists(&self) -> Vec<Effect> {
        if !self.connection.connected {
            return Vec::new();
        }
        let mut effects = vec![Effect::execute(op::ListSessions::new(
            (*self.lists.list_query).clone(),
        ))];
        effects.extend(self.lists.settlement.expanded.iter().map(|project_id| {
            Effect::execute(op::ReadSettledSessions {
                project_id: project_id.clone(),
                limit: self
                    .lists
                    .settlement
                    .pages
                    .get(project_id)
                    .map_or(5, |page| page.limit),
            })
        }));
        effects
    }
}

fn update_page(
    page: &mut Arc<ThreadList>,
    id: &crate::session::SessionRef,
    settled: bool,
    revision: u64,
) {
    if let Some(index) = page
        .data
        .iter()
        .position(|row| row.id.as_ref() == Some(id) && row.list_revision <= revision)
    {
        let row = &mut Arc::make_mut(page).data[index];
        row.settled = settled;
        row.list_revision = revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::operations::{ListSessions, Operation, ReadSettledSessions};
    use agent_protocol::{
        operations::SessionSettlement,
        session::{ProviderKind, SessionRef},
    };

    fn page(row: Thread, revision: u64) -> ThreadList {
        serde_json::from_value(serde_json::json!({"revision":revision,"data":[row],"projects":[],"limit":15,"hasMore":false,"hasMoreProjects":false,"projectPages":{}})).unwrap()
    }

    #[test]
    fn stale_reads_cannot_reverse_settlement_or_change_navigation_drafts_and_unread() {
        let id = SessionRef::new(ProviderKind::Codex, "thread".into()).unwrap();
        let row = Thread {
            id: Some(id.clone()),
            status: crate::models::SessionStatus::Idle,
            ..Default::default()
        };
        let mut state = Snapshot {
            connection: Connection {
                connected: true,
                ..Default::default()
            },
            lists: Lists {
                threads: Some(Arc::new(page(row.clone(), 1))),
                ..Default::default()
            },
            ..Default::default()
        };
        Arc::make_mut(&mut state.sessions.conversations).insert(id.clone(), Arc::new(row.clone()));
        Arc::make_mut(&mut Arc::make_mut(&mut state.pane).navigation).thread_id = Some(id.clone());
        Arc::make_mut(&mut state.lists.activity)
            .unread
            .insert(id.clone());
        let navigation = state.pane.navigation.clone();
        let drafts = state.input.drafts.clone();
        let effects = state.accept_settlement(SessionSettlement {
            session: id.clone(),
            settled: true,
            revision: 3,
        });
        assert!(!effects.is_empty());
        assert!(state.sessions.conversations[&id].settled);
        assert_eq!(state.pane.navigation, navigation);
        assert_eq!(state.input.drafts, drafts);
        assert!(state.lists.activity.unread.contains(&id));
        ListSessions::new(Default::default()).apply(&mut state, page(row.clone(), 1));
        assert!(state.lists.threads.as_ref().unwrap().data[0].settled);
        assert!(state.thread_list().unwrap().threads.is_empty());
        ReadSettledSessions {
            project_id: None,
            limit: 15,
        }
        .apply(&mut state, page(row.clone(), 1));
        assert!(state.lists.settlement.pages[&None].data[0].settled);
        state.accept_settlement(SessionSettlement {
            session: id.clone(),
            settled: false,
            revision: 4,
        });
        let mut old = row;
        old.settled = true;
        old.list_revision = 3;
        ReadSettledSessions {
            project_id: None,
            limit: 15,
        }
        .apply(&mut state, page(old, 3));
        assert!(state.thread_list().unwrap().settled[0].threads.is_empty());
        assert!(
            state
                .accept_settlement(SessionSettlement {
                    session: id,
                    settled: true,
                    revision: 3
                })
                .is_empty()
        );
    }
    #[test]
    fn a_late_rpc_reply_cannot_override_a_newer_metadata_observation() {
        let id = SessionRef::new(ProviderKind::Claude, "thread".into()).unwrap();
        let mut state = Snapshot::default();
        let mut rows = [Thread {
            id: Some(id.clone()),
            settled: false,
            list_revision: 9,
            ..Default::default()
        }];
        state.reconcile_settlements(&mut rows);
        assert!(
            state
                .accept_settlement(SessionSettlement {
                    session: id.clone(),
                    settled: true,
                    revision: 8
                })
                .is_empty()
        );
        rows[0].list_revision = 1;
        rows[0].settled = true;
        state.reconcile_settlements(&mut rows);
        assert!(!rows[0].settled);
        assert_eq!(
            rows[0].list_revision, 1,
            "organization must not make stale metadata appear newer"
        );
    }
    #[test]
    fn settled_rows_keep_local_project_placement_and_live_metadata() {
        let id = SessionRef::new(ProviderKind::Claude, "thread".into()).unwrap();
        let row = Thread {
            id: Some(id.clone()),
            name: Some("Original".into()),
            settled: true,
            list_revision: 1,
            ..Default::default()
        };
        let mut root = page(row.clone(), 1);
        root.data.clear();
        root.projects.push(crate::models::Project {
            id: "p".into(),
            name: "Project".into(),
            ..Default::default()
        });
        let mut state = Snapshot::default();
        state.lists.threads = Some(Arc::new(root));
        Arc::make_mut(&mut state.lists.expanded_projects).insert("p".into(), 5);
        Arc::make_mut(&mut state.lists.settlement)
            .pages
            .insert(None, Arc::new(page(row.clone(), 1)));
        let (moved, _) = reduce(
            &state,
            Event::Intent(Intent::MoveChatToProject {
                thread_id: id.clone(),
                project_id: "p".into(),
            }),
        );
        let view = moved.thread_list().unwrap();
        assert!(view.settled[0].threads.is_empty());
        let shelf = view.projects[0].settled.as_ref().unwrap();
        assert_eq!(shelf.threads[0].id, id);
        assert_eq!(shelf.threads[0].project_id.as_deref(), Some("p"));
        let (renamed, _) = super::super::notifications::notification(
            &moved,
            crate::protocol::Notification::SessionRenamed {
                session: id.clone(),
                name: Some("Renamed".into()),
                revision: 2,
            },
        );
        assert_eq!(
            renamed.thread_list().unwrap().projects[0]
                .settled
                .as_ref()
                .unwrap()
                .threads[0]
                .title,
            "Renamed"
        );
        let (running, _) = super::super::notifications::notification(
            &renamed,
            crate::protocol::Notification::SessionUpdated {
                thread: Box::new(Thread {
                    status: crate::models::SessionStatus::Running,
                    list_revision: 3,
                    ..row
                }),
                project: None,
            },
        );
        assert!(
            running.thread_list().unwrap().projects[0]
                .settled
                .as_ref()
                .unwrap()
                .threads[0]
                .active
        );
        let disconnected = reduce(&running, Event::Disconnected).0;
        assert!(
            !disconnected.thread_list().unwrap().projects[0]
                .settled
                .as_ref()
                .unwrap()
                .threads[0]
                .active
        );
        assert!(disconnected.thread_metadata(&id).unwrap().settled);
    }
}
