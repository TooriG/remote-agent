//! The composer's folder menu, as the desktop client's: where the next chat
//! starts. Clients present it natively and act on the chosen item.
use super::list::ThreadList;

#[derive(Clone, Debug, PartialEq)]
pub enum FolderChoice {
    /// Starts a new chat in `path`; an empty path is the Host's chat folder,
    /// which belongs to no project. `checked` marks the draft's folder.
    Folder {
        path: String,
        title: String,
        checked: bool,
    },
    /// Loads the next page of projects.
    MoreProjects,
    /// Adds a folder the user picks as a project and starts a chat in it.
    ChooseFolder,
}

/// The unassigned chat, each loaded project's roots, more projects while the
/// Host has them, and another folder where this device can pick one (`local`).
/// A project with one root is named by the project; a root of several, by its path.
pub fn folder_choices(list: Option<&ThreadList>, selected: &str, local: bool) -> Vec<FolderChoice> {
    let folder = |path: &str, title: &str| FolderChoice::Folder {
        path: path.into(),
        title: title.into(),
        checked: path == selected,
    };
    let mut choices = vec![folder("", "チャット")];
    for project in list.into_iter().flat_map(|list| &list.projects) {
        for root in &project.roots {
            let title = if project.roots.len() == 1 {
                &project.name
            } else {
                &root.path
            };
            choices.push(folder(&root.path, title));
        }
    }
    if list.is_some_and(|list| list.has_more_projects) {
        choices.push(FolderChoice::MoreProjects);
    }
    if local {
        choices.push(FolderChoice::ChooseFolder);
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::ProjectRoot, presentation::list::ProjectSummary};

    fn project(name: &str, roots: &[&str]) -> ProjectSummary {
        ProjectSummary {
            host_id: "local".into(),
            id: name.into(),
            name: name.into(),
            coordinator_url: None,
            roots: roots
                .iter()
                .map(|path| ProjectRoot {
                    path: (*path).into(),
                })
                .collect(),
            icon: None,
            monogram: String::new(),
            icon_color: 0,
            pinned: false,
            section: None,
            chats_archived: false,
            expanded: false,
            loading: false,
            error: None,
            threads: Vec::new(),
            has_more: false,
        }
    }

    fn list(projects: Vec<ProjectSummary>, has_more_projects: bool) -> ThreadList {
        ThreadList {
            loading_projects: false,
            loading_chats: false,
            notice: None,
            threads: Vec::new(),
            projects,
            hidden_projects: vec![project("非表示", &["/hidden"])],
            has_more: false,
            has_more_projects,
        }
    }

    fn folder(path: &str, title: &str, checked: bool) -> FolderChoice {
        FolderChoice::Folder {
            path: path.into(),
            title: title.into(),
            checked,
        }
    }

    #[test]
    fn projects_follow_the_unassigned_chat_and_the_draft_folder_is_checked() {
        let list = list(
            vec![
                project("日本語のプロジェクト 👋", &["/one"]),
                project("複数", &["/two/a", "/two/b"]),
            ],
            true,
        );
        assert_eq!(
            folder_choices(Some(&list), "/two/b", true),
            vec![
                folder("", "チャット", false),
                folder("/one", "日本語のプロジェクト 👋", false),
                folder("/two/a", "/two/a", false),
                folder("/two/b", "/two/b", true),
                FolderChoice::MoreProjects,
                FolderChoice::ChooseFolder,
            ],
            "hidden projects stay out of the menu"
        );
        assert_eq!(
            folder_choices(Some(&list), "", false)[0],
            folder("", "チャット", true)
        );
    }

    #[test]
    fn more_projects_and_another_folder_appear_only_when_available() {
        let loaded = list(vec![project("一つ", &["/one"])], false);
        assert_eq!(
            folder_choices(Some(&loaded), "/elsewhere", false),
            vec![folder("", "チャット", false), folder("/one", "一つ", false)],
            "a folder outside the projects checks nothing"
        );
        assert_eq!(
            folder_choices(None, "", true),
            vec![folder("", "チャット", true), FolderChoice::ChooseFolder],
            "before the list loads the chat and another folder remain"
        );
    }
}
