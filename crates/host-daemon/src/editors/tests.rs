use super::*;
use std::path::Path;

#[test]
fn discovery_reuses_a_pending_scan_and_retries_closed_failed_or_expired_scans() {
    let now = Instant::now();
    assert!(!scan_expired(&None, false, now));
    assert!(scan_expired(&None, true, now));
    assert!(scan_expired(&Some((now, Err("failed".into()))), false, now));
    let success = Some((now, Ok(FileOpenCapabilities::default())));
    assert!(!scan_expired(&success, true, now + Duration::from_secs(59)));
    assert!(scan_expired(&success, true, now + Duration::from_secs(60)));
}

#[test]
fn vscode_and_cursor_use_goto_and_preserve_the_file_position() {
    assert_eq!(
        launch::editor_args(
            EditorId::VsCode,
            Path::new("/repo/a b.rs"),
            Some(12),
            Some(3),
            &[]
        )
        .unwrap(),
        ["--goto", "/repo/a b.rs:12:3"]
    );
    assert_eq!(
        launch::editor_args(
            EditorId::Cursor,
            Path::new("/repo/a.rs"),
            Some(12),
            None,
            &base_args(EditorId::Cursor)
        )
        .unwrap(),
        ["--classic", "--goto", "/repo/a.rs:12"]
    );
    assert_eq!(
        launch::editor_args(
            EditorId::Kiro,
            Path::new("/repo"),
            None,
            None,
            &base_args(EditorId::Kiro)
        )
        .unwrap(),
        ["ide", "/repo"]
    );
}

#[test]
fn jetbrains_columns_are_zero_based_and_zed_uses_a_direct_position() {
    assert_eq!(
        launch::editor_args(
            EditorId::Idea,
            Path::new("/repo/a.rs"),
            Some(12),
            Some(3),
            &[]
        )
        .unwrap(),
        ["--line", "12", "--column", "2", "/repo/a.rs"]
    );
    assert_eq!(
        launch::editor_args(
            EditorId::Pycharm,
            Path::new("/repo/a.py"),
            Some(1),
            Some(1),
            &[]
        )
        .unwrap(),
        ["--line", "1", "--column", "0", "/repo/a.py"]
    );
    assert_eq!(
        launch::editor_args(
            EditorId::Zed,
            Path::new("/repo/a.rs"),
            Some(12),
            Some(3),
            &[]
        )
        .unwrap(),
        ["/repo/a.rs:12:3"]
    );
}

#[test]
fn every_catalog_editor_has_a_launcher_and_supported_positions() {
    for editor in EditorId::ALL {
        if *editor != EditorId::FileManager {
            assert!(!commands(*editor).is_empty());
        }
        let args =
            launch::editor_args(*editor, Path::new("/repo"), None, None, &base_args(*editor))
                .unwrap();
        assert_eq!(args.last().unwrap(), "/repo");
    }
}

#[tokio::test]
async fn a_failed_launch_reports_failure_without_falling_back_to_a_different_application() {
    let root = tempfile::tempdir().unwrap();
    let result = launch::spawn(&root.path().join("missing-editor"), &["/repo/a.rs".into()]).await;
    assert!(result.unwrap_err().contains("Unable to launch"));
}
