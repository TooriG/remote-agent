mod chip_color;
pub use chip_color::mobile_file_chip_color;
mod editor;
pub use editor::{FileEditorPresentation, file_edit_prompt, file_line_start_utf16};
mod icons;
mod mobile;
mod mobile_icons;
mod preview;
pub use mobile::{
    MobileFileAction, MobileFileMenuItem, mobile_file_menu, mobile_file_open_request,
};
pub use preview::{
    file_preview_mime_type, file_preview_truncation_notice, image_extension, is_image_file,
};

#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn mobile_file_icon(path: String) -> String {
    mobile_icons::icon(&path).into()
}

pub mod tree;
pub use icons::desktop_file_icon;
pub use tree::{
    FileTreeAction, FileTreeKey, FileTreeRequest, FileTreeRow, FileTreeSurface, FileTreeView,
    FileTrees, file_tree_path,
};

#[cfg(feature = "bindings")]
#[derive(Clone, uniffi::Object)]
pub struct Files {
    pub(crate) resources: crate::state::Files,
    pub(crate) selection: std::sync::Arc<crate::state::WorkspaceLocation>,
}
#[cfg(feature = "bindings")]
impl crate::state::Snapshot {
    pub fn files_view(&self) -> std::sync::Arc<Files> {
        std::sync::Arc::new(Files {
            resources: self.files.clone(),
            selection: self.pane.workspace.clone(),
        })
    }
}
