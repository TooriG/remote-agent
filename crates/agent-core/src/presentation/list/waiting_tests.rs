use super::*;
use crate::{
    models::Thread,
    session::{ProviderKind, SessionRef},
};

fn session(provider: ProviderKind, index: usize) -> SessionRef {
    SessionRef::new(provider, format!("task{index}")).unwrap()
}

#[test]
fn waiting_membership_keeps_provider_identity_duplicates_and_absent_rows_at_every_size() {
    for count in [0, 1, 16, 31, 32, 63, 64, 65, 128, 512] {
        let rows: Vec<_> = (0..count)
            .flat_map(|index| {
                [ProviderKind::Codex, ProviderKind::Claude].map(|provider| Thread {
                    id: Some(session(provider, index)),
                    name: Some(format!("Task {index}")),
                    ..Default::default()
                })
            })
            .collect();
        let mut waiting: Vec<_> = (0..count)
            .flat_map(|index| {
                let mut sessions = Vec::new();
                if index % 3 == 0 {
                    sessions.push(session(ProviderKind::Codex, index));
                }
                if index % 5 == 0 {
                    sessions.push(session(ProviderKind::Claude, index));
                }
                sessions
            })
            .collect();
        waiting.extend((0..100).map(|index| session(ProviderKind::Codex, count + index)));
        waiting.extend([
            session(ProviderKind::Codex, 0),
            session(ProviderKind::Codex, 0),
        ]);
        for waiting in [waiting.as_slice(), &[]] {
            let summaries = thread_summaries(
                crate::localization::UiLanguage::Japanese,
                rows.iter(),
                &BTreeMap::new(),
                &BTreeSet::new(),
                TaskMarks {
                    waiting,
                    ..Default::default()
                },
                &HashSet::new(),
                None,
            );
            assert_eq!(summaries.len(), count * 2);
            for (index, pair) in summaries.as_chunks::<2>().0.iter().enumerate() {
                assert_eq!(pair[0].waiting, !waiting.is_empty() && index % 3 == 0);
                assert_eq!(pair[1].waiting, !waiting.is_empty() && index % 5 == 0);
            }
        }
        assert!(
            thread_summaries(
                crate::localization::UiLanguage::Japanese,
                std::iter::empty(),
                &BTreeMap::new(),
                &BTreeSet::new(),
                TaskMarks {
                    waiting: &waiting,
                    ..Default::default()
                },
                &HashSet::new(),
                None
            )
            .is_empty()
        );
    }
}
