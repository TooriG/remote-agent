use super::*;

pub use agent_protocol::operations::SetSessionSettled;

impl Operation for SetSessionSettled {
    rpc_operation!();
    fn navigation_bound(&self) -> bool {
        false
    }
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::Settlement {
            session: self.thread_id.clone(),
        })
    }
    fn apply(self, snapshot: &mut Snapshot, change: Self::Output) -> Vec<Effect> {
        snapshot.accept_settlement(change)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadSettledSessions {
    pub project_id: Option<String>,
    pub limit: u32,
}

impl Operation for ReadSettledSessions {
    const BACKGROUND: bool = true;
    no_input!();
    type Output = crate::models::ThreadList;
    fn key(&self) -> Option<OperationKey> {
        Some(OperationKey::SettledList {
            project_id: self.project_id.clone(),
        })
    }
    fn scheduling(&self) -> Scheduling {
        Scheduling::LatestHostState(self.key().unwrap())
    }
    async fn run(&self, _: (), context: &mut Execution<'_>) -> Result<Self::Output, PeerError> {
        if let Some(project_id) = &self.project_id {
            context
                .call(&ListProjectSessions {
                    project_id: project_id.clone(),
                    limit: self.limit,
                    search_term: String::new(),
                    settled: true,
                })
                .await
        } else {
            context
                .call(&ListSessions::new(crate::models::ListQuery {
                    settled: true,
                    limit: self.limit,
                    part: Some(crate::models::ListPart::Chats),
                    project_limits: Some(Default::default()),
                    ..Default::default()
                }))
                .await
        }
    }
    fn apply(self, snapshot: &mut Snapshot, mut output: Self::Output) -> Vec<Effect> {
        snapshot.reconcile_settlements(&mut output.data);
        super::lists::update_titles(snapshot, &output.data);
        let decorations = super::lists::decorations(
            agent_protocol::operations::ListDecorationScope::Settled {
                project_id: self.project_id.clone(),
            },
            &[],
            &output.data,
        );
        Arc::make_mut(&mut snapshot.lists.settlement)
            .pages
            .insert(self.project_id, Arc::new(output));
        decorations.into_iter().collect()
    }
}
