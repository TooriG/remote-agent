/// Host paths are independent of the client's OS. Android's native path parser,
/// for example, does not recognize Windows drive and UNC paths as absolute.
pub fn is_absolute_host_path(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    if path.starts_with('/') {
        return true;
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
