//! The work a conversation waits on after its agent answered, as T3 Code's
//! composer strip names it:
//! https://github.com/pingdotgg/t3code/blob/dac8b11ddc25bd5da62a367fdb6c3de777ae2986/packages/client-runtime/src/state/threadExecution.ts#L290
use crate::localization::UiLanguage;
use agent_protocol::execution::{
    BackgroundOutcome, BackgroundReport, BackgroundTask, BackgroundTaskKind,
};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct BackgroundWork {
    /// Grouped by kind: agents first, loose tasks last.
    pub items: Vec<BackgroundWorkItem>,
    /// True when the work will wake the agent (subagents, monitors). False
    /// when only commands remain, such as a dev server: the agent is done.
    pub waiting: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct BackgroundWorkItem {
    pub id: String,
    pub kind: BackgroundTaskKind,
    /// The provider's name for the work, verbatim.
    pub description: Option<String>,
}

impl BackgroundWork {
    pub fn new(tasks: &[BackgroundTask]) -> Option<Self> {
        if tasks.is_empty() {
            return None;
        }
        let mut items: Vec<_> = tasks
            .iter()
            .map(|task| BackgroundWorkItem {
                id: task.id.clone(),
                kind: task.kind,
                description: task
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
            })
            .collect();
        items.sort_by_key(|item| item.kind);
        Some(Self {
            items,
            waiting: agent_protocol::execution::background_holds_completion(tasks),
        })
    }

    /// "Waiting on subagent Review src/math.ts", "Waiting on 2 subagents and
    /// 1 command", or "Running: Start the dev server" when only commands remain.
    pub fn title(&self, language: UiLanguage) -> String {
        use UiLanguage::*;
        if let [only] = self.items.as_slice() {
            let noun = noun(only.kind, language, 1);
            return match (language, self.waiting, only.description.as_deref()) {
                (English, true, Some(label)) => format!("Waiting on {noun} {label}"),
                (English, true, None) => format!("Waiting on a {noun}"),
                (English, false, Some(label)) => format!("Running: {label}"),
                (English, false, None) => format!("Running a {noun}"),
                (Japanese, true, Some(label)) => format!("{noun}「{label}」を待機中"),
                (Japanese, true, None) => format!("{noun}を待機中"),
                (Japanese, false, Some(label)) => format!("実行中: {label}"),
                (Japanese, false, None) => format!("{noun}を実行中"),
            };
        }
        let mut groups: Vec<(BackgroundTaskKind, usize)> = Vec::new();
        for item in &self.items {
            match groups.last_mut() {
                Some((kind, count)) if *kind == item.kind => *count += 1,
                _ => groups.push((item.kind, 1)),
            }
        }
        let groups: Vec<_> = groups
            .into_iter()
            .map(|(kind, count)| match language {
                English => format!("{count} {}", noun(kind, language, count)),
                Japanese => format!("{} {count}件", noun(kind, language, count)),
            })
            .collect();
        match (language, self.waiting) {
            (English, true) => format!("Waiting on {}", join_with_and(&groups)),
            (English, false) => format!("Running {}", join_with_and(&groups)),
            (Japanese, true) => format!("{}を待機中", groups.join("、")),
            (Japanese, false) => format!("{}を実行中", groups.join("、")),
        }
    }

    /// Each item's name, or its noun when the provider gave none. A single
    /// item is already in the title.
    pub fn description(&self, language: UiLanguage) -> Option<String> {
        if self.items.len() < 2 {
            return None;
        }
        let labels: Vec<_> = self
            .items
            .iter()
            .map(|item| {
                item.description
                    .clone()
                    .unwrap_or_else(|| noun(item.kind, language, 1).into())
            })
            .collect();
        Some(labels.join(match language {
            UiLanguage::English => ", ",
            UiLanguage::Japanese => "、",
        }))
    }
}

fn noun(kind: BackgroundTaskKind, language: UiLanguage, count: usize) -> &'static str {
    use BackgroundTaskKind::{Command, Monitor, Subagent};
    const UNNAMED: BackgroundTaskKind = BackgroundTaskKind::BackgroundTask;
    match (language, kind, count == 1) {
        (UiLanguage::English, Subagent, true) => "subagent",
        (UiLanguage::English, Subagent, false) => "subagents",
        (UiLanguage::English, Command, true) => "command",
        (UiLanguage::English, Command, false) => "commands",
        (UiLanguage::English, Monitor, true) => "monitor",
        (UiLanguage::English, Monitor, false) => "monitors",
        (UiLanguage::English, UNNAMED, true) => "background task",
        (UiLanguage::English, UNNAMED, false) => "background tasks",
        (UiLanguage::Japanese, Subagent, _) => "サブエージェント",
        (UiLanguage::Japanese, Command, _) => "コマンド",
        (UiLanguage::Japanese, Monitor, _) => "モニター",
        (UiLanguage::Japanese, UNNAMED, _) => "バックグラウンドタスク",
    }
}

/// What the background work a notification reports did, as T3 Code's
/// notification summary names it.
pub(crate) fn notification_title(reports: &[BackgroundReport]) -> String {
    let named = |report: &BackgroundReport| {
        let noun = noun(report.kind, UiLanguage::Japanese, 1);
        match report_label(report) {
            Some(label) => format!("{noun}「{label}」"),
            None => noun.to_owned(),
        }
    };
    let verb = match combined_outcome(reports) {
        BackgroundOutcome::Failed => "が失敗しました",
        BackgroundOutcome::Cancelled => "が停止されました",
        BackgroundOutcome::Updated
            if reports
                .iter()
                .all(|report| report.kind == BackgroundTaskKind::Monitor) =>
        {
            "が新しい出力を報告しました"
        }
        BackgroundOutcome::Updated => "が更新されました",
        BackgroundOutcome::Completed | BackgroundOutcome::Unknown => "が完了しました",
    };
    match reports {
        [] => format!("バックグラウンドの作業{verb}"),
        [only] => format!("{}{verb}", named(only)),
        _ if reports.len() > 3 || reports.iter().all(|report| report_label(report).is_none()) => {
            let kind = reports[0].kind;
            let noun = if reports.iter().all(|report| report.kind == kind) {
                noun(kind, UiLanguage::Japanese, 2)
            } else {
                noun(BackgroundTaskKind::BackgroundTask, UiLanguage::Japanese, 2)
            };
            format!("{noun} {}件{verb}", reports.len())
        }
        _ => format!(
            "{}{verb}",
            reports.iter().map(named).collect::<Vec<_>>().join("、")
        ),
    }
}

/// A report's name on one line, shortened as T3 Code's.
fn report_label(report: &BackgroundReport) -> Option<String> {
    const MAX: usize = 80;
    let line = report.label.as_deref()?.trim().lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    Some(if line.chars().count() > MAX {
        line.chars().take(MAX - 1).chain(['…']).collect()
    } else {
        line.to_owned()
    })
}

fn combined_outcome(reports: &[BackgroundReport]) -> BackgroundOutcome {
    let has = |outcome| reports.iter().any(|report| report.outcome == outcome);
    if has(BackgroundOutcome::Failed) {
        BackgroundOutcome::Failed
    } else if has(BackgroundOutcome::Cancelled) {
        BackgroundOutcome::Cancelled
    } else if !reports.is_empty()
        && reports
            .iter()
            .all(|report| report.outcome == BackgroundOutcome::Completed)
    {
        BackgroundOutcome::Completed
    } else if has(BackgroundOutcome::Updated) {
        BackgroundOutcome::Updated
    } else {
        BackgroundOutcome::Unknown
    }
}

fn join_with_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BackgroundTaskKind::{Command, Monitor, Subagent};
    use UiLanguage::*;

    fn work(tasks: &[(BackgroundTaskKind, Option<&str>)]) -> BackgroundWork {
        BackgroundWork::new(
            &tasks
                .iter()
                .enumerate()
                .map(|(index, (kind, description))| BackgroundTask {
                    id: index.to_string(),
                    kind: *kind,
                    description: description.map(str::to_owned),
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn notifications_name_what_the_work_did_as_t3_code() {
        use BackgroundOutcome::*;
        let report = |kind, label: Option<&str>, outcome| BackgroundReport {
            kind,
            label: label.map(str::to_owned),
            outcome,
        };
        for (reports, title) in [
            (
                vec![report(Command, Some("npm run dev\nmore"), Completed)],
                "コマンド「npm run dev」が完了しました",
            ),
            (
                vec![report(Monitor, Some("Watch the build"), Updated)],
                "モニター「Watch the build」が新しい出力を報告しました",
            ),
            (
                vec![report(Subagent, None, Cancelled)],
                "サブエージェントが停止されました",
            ),
            (
                vec![
                    report(Subagent, Some("Review"), Completed),
                    report(Command, Some("npm test"), Failed),
                ],
                "サブエージェント「Review」、コマンド「npm test」が失敗しました",
            ),
            (
                vec![
                    report(Command, None, Completed),
                    report(Command, None, Unknown),
                ],
                "コマンド 2件が完了しました",
            ),
            (
                vec![
                    report(Command, Some("a"), Completed),
                    report(Monitor, Some("b"), Completed),
                    report(Subagent, Some("c"), Completed),
                    report(Command, Some("d"), Completed),
                ],
                "バックグラウンドタスク 4件が完了しました",
            ),
        ] {
            assert_eq!(notification_title(&reports), title);
        }
    }

    #[test]
    fn nothing_running_presents_no_work() {
        assert_eq!(BackgroundWork::new(&[]), None);
    }

    #[test]
    fn a_single_item_names_itself_in_the_title_as_t3_code() {
        for (task, english, japanese) in [
            (
                (Subagent, Some("Review src/math.ts")),
                "Waiting on subagent Review src/math.ts",
                "サブエージェント「Review src/math.ts」を待機中",
            ),
            ((Monitor, None), "Waiting on a monitor", "モニターを待機中"),
            (
                (Command, Some(" Start the dev server ")),
                "Running: Start the dev server",
                "実行中: Start the dev server",
            ),
            (
                (Command, Some("  ")),
                "Running a command",
                "コマンドを実行中",
            ),
            (
                (BackgroundTaskKind::BackgroundTask, None),
                "Waiting on a background task",
                "バックグラウンドタスクを待機中",
            ),
        ] {
            let work = work(&[task]);
            assert_eq!(work.title(English), english);
            assert_eq!(work.title(Japanese), japanese);
            assert_eq!(work.description(English), None);
        }
    }

    #[test]
    fn several_items_group_by_kind_with_agents_first_and_list_their_names() {
        let work = work(&[
            (Command, Some("npm run dev")),
            (Monitor, None),
            (Subagent, Some("Explore")),
            (Subagent, Some("Review")),
        ]);
        assert!(work.waiting, "subagents and monitors wake the agent");
        assert_eq!(
            work.title(English),
            "Waiting on 2 subagents, 1 command and 1 monitor"
        );
        assert_eq!(
            work.title(Japanese),
            "サブエージェント 2件、コマンド 1件、モニター 1件を待機中"
        );
        assert_eq!(
            work.description(English).as_deref(),
            Some("Explore, Review, npm run dev, monitor")
        );
        assert_eq!(
            work.description(Japanese).as_deref(),
            Some("Explore、Review、npm run dev、モニター")
        );
        let commands = super::BackgroundWork::new(&[
            BackgroundTask {
                id: "a".into(),
                kind: Command,
                description: None,
            },
            BackgroundTask {
                id: "b".into(),
                kind: Command,
                description: None,
            },
        ])
        .unwrap();
        assert!(
            !commands.waiting,
            "a dev server does not keep the agent working"
        );
        assert_eq!(commands.title(English), "Running 2 commands");
        assert_eq!(commands.title(Japanese), "コマンド 2件を実行中");
    }
}
