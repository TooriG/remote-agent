//! Host editor identities and RPC contracts. Executable discovery and launching belong to the Host.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditorId {
    Cursor,
    Trae,
    Kiro,
    #[serde(rename = "vscode")]
    VsCode,
    #[serde(rename = "vscode-insiders")]
    VsCodeInsiders,
    #[serde(rename = "vscodium")]
    VsCodium,
    Zed,
    Antigravity,
    Idea,
    Aqua,
    Clion,
    Datagrip,
    Dataspell,
    Goland,
    Phpstorm,
    Pycharm,
    Rider,
    Rubymine,
    Rustrover,
    Webstorm,
    FileManager,
}

impl EditorId {
    // T3's contracts/editor.ts order is also its preferred-editor fallback order.
    pub const ALL: &[Self] = &[
        Self::Cursor,
        Self::Trae,
        Self::Kiro,
        Self::VsCode,
        Self::VsCodeInsiders,
        Self::VsCodium,
        Self::Zed,
        Self::Antigravity,
        Self::Idea,
        Self::Aqua,
        Self::Clion,
        Self::Datagrip,
        Self::Dataspell,
        Self::Goland,
        Self::Phpstorm,
        Self::Pycharm,
        Self::Rider,
        Self::Rubymine,
        Self::Rustrover,
        Self::Webstorm,
        Self::FileManager,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Trae => "Trae",
            Self::Kiro => "Kiro",
            Self::VsCode => "VS Code",
            Self::VsCodeInsiders => "VS Code Insiders",
            Self::VsCodium => "VSCodium",
            Self::Zed => "Zed",
            Self::Antigravity => "Antigravity",
            Self::Idea => "IntelliJ IDEA",
            Self::Aqua => "Aqua",
            Self::Clion => "CLion",
            Self::Datagrip => "DataGrip",
            Self::Dataspell => "DataSpell",
            Self::Goland => "GoLand",
            Self::Phpstorm => "PhpStorm",
            Self::Pycharm => "PyCharm",
            Self::Rider => "Rider",
            Self::Rubymine => "RubyMine",
            Self::Rustrover => "RustRover",
            Self::Webstorm => "WebStorm",
            Self::FileManager => "File Manager",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileManagerRevealKind {
    Finder,
    FileExplorer,
    Files,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOpenCapabilities {
    pub editors: Vec<EditorId>,
    pub reveal: Option<FileManagerRevealKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileOpenCapabilities {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInEditor {
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub editor: EditorId,
    pub reveal: bool,
}

impl OpenInEditor {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.path.trim().is_empty() || self.path.contains('\0') {
            return Err("file path is required");
        }
        if self.reveal && self.editor != EditorId::FileManager {
            return Err("reveal is only supported by the file manager");
        }
        if self.line == Some(0) || self.column == Some(0) {
            return Err("file positions are one-based");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_contract_ids_and_order_match_t3() {
        let ids = EditorId::ALL
            .iter()
            .map(|id| serde_json::to_value(id).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(ids),
            serde_json::json!([
                "cursor",
                "trae",
                "kiro",
                "vscode",
                "vscode-insiders",
                "vscodium",
                "zed",
                "antigravity",
                "idea",
                "aqua",
                "clion",
                "datagrip",
                "dataspell",
                "goland",
                "phpstorm",
                "pycharm",
                "rider",
                "rubymine",
                "rustrover",
                "webstorm",
                "file-manager"
            ])
        );
    }

    #[test]
    fn reveal_is_distinct_from_open_and_requires_a_file_manager() {
        let mut request = OpenInEditor {
            path: "~/notes.md".into(),
            line: Some(12),
            column: Some(3),
            editor: EditorId::VsCode,
            reveal: false,
        };
        assert!(request.validate().is_ok());
        request.reveal = true;
        assert!(request.validate().is_err());
        request.editor = EditorId::FileManager;
        assert!(request.validate().is_ok());
        request.line = Some(0);
        assert!(request.validate().is_err());
    }
}
