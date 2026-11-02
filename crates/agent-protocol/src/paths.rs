/// Host paths are independent of the client's OS. Android's native path parser,
/// for example, does not recognize Windows drive and UNC paths as absolute.
pub fn is_absolute_host_path(path: &str) -> bool {
    if path.starts_with('/') {
        return !path.contains('\0');
    }
    is_windows_absolute_host_path(path)
}

pub fn is_windows_absolute_host_path(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        return true;
    }
    if let Some(unc) = path.strip_prefix(r"\\") {
        let mut components = unc.split(['\\', '/']);
        return components.next().is_some_and(|server| !server.is_empty())
            && components.next().is_some_and(|share| !share.is_empty());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_paths_are_classified_without_using_the_clients_platform() {
        for path in [
            "C:/workspace",
            r"D:\workspace",
            r"\\server\share",
            r"\\server/share/dir",
        ] {
            assert!(is_absolute_host_path(path));
            assert!(is_windows_absolute_host_path(path));
        }
        for path in ["/workspace", "/:data"] {
            assert!(is_absolute_host_path(path));
            assert!(!is_windows_absolute_host_path(path));
        }
        for path in [
            "",
            "C:",
            "relative",
            r"\drive-relative",
            r"\\server",
            "C:/null\0",
        ] {
            assert!(!is_absolute_host_path(path));
            assert!(!is_windows_absolute_host_path(path));
        }
    }
}
