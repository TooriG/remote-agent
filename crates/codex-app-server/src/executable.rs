use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Error,
    platform::{bundled_codex_path, executable_path, is_executable},
};

pub(crate) const DEFAULT_CODEX_PROGRAM: &str = "codex";

pub(crate) fn resolve(program: &Path) -> Result<PathBuf, Error> {
    let path = env::var_os("PATH");
    resolve_from(program, path.as_deref(), bundled_codex_path())
}

fn resolve_from(
    program: &Path,
    path: Option<&OsStr>,
    bundled_codex: Option<&Path>,
) -> Result<PathBuf, Error> {
    if program.components().count() > 1 || program.is_absolute() {
        let resolved = fs::canonicalize(executable_path(program)).map_err(|source| {
            Error::ResolveExecutable {
                program: program.to_path_buf(),
                source,
            }
        })?;
        if is_executable(&resolved) {
            return Ok(resolved);
        }
        return Err(Error::ExecutableNotFound(program.to_path_buf()));
    }

    if let Some(path) = path {
        for directory in env::split_paths(path) {
            let candidate = directory.join(executable_path(program));
            if let Some(resolved) = resolve_candidate(Some(&candidate))? {
                return Ok(resolved);
            }
        }
    }
    if program == Path::new(DEFAULT_CODEX_PROGRAM)
        && let Some(resolved) = resolve_candidate(bundled_codex)?
    {
        return Ok(resolved);
    }
    Err(Error::ExecutableNotFound(program.to_path_buf()))
}

fn resolve_candidate(candidate: Option<&Path>) -> Result<Option<PathBuf>, Error> {
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    if !is_executable(candidate) {
        return Ok(None);
    }
    fs::canonicalize(candidate)
        .map(Some)
        .map_err(|source| Error::ResolveExecutable {
            program: candidate.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write_executable(path: &Path) {
        fs::write(path, b"not invoked by this test").expect("write test executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
    }

    fn path_variable(directory: &Path) -> std::ffi::OsString {
        env::join_paths([directory]).expect("build test PATH")
    }

    #[test]
    fn resolves_explicit_then_path_then_bundled_and_reports_missing_candidates() {
        let root = tempfile::tempdir().unwrap();
        let bundled = root
            .path()
            .join("ChatGPT.app/Contents/Resources/codex-cli/bin/codex");
        let path_directory = root.path().join("path");
        let explicit = root.path().join(executable_path(Path::new("custom-codex")));
        fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        fs::create_dir(&path_directory).unwrap();
        let path_codex = path_directory.join(executable_path(Path::new("codex")));
        let competing = path_directory.join(executable_path(Path::new("custom-codex")));
        for path in [&explicit, &bundled, &path_codex, &competing] {
            write_executable(path);
        }
        let path = path_variable(&path_directory);
        assert_eq!(
            resolve_from(&explicit, Some(&path), Some(&bundled)).unwrap(),
            fs::canonicalize(&explicit).unwrap()
        );
        let relative_root = tempfile::tempdir_in(".").unwrap();
        let relative = Path::new(relative_root.path().file_name().unwrap())
            .join(executable_path(Path::new("codex")));
        write_executable(&relative);
        assert_eq!(
            resolve_from(&relative, Some(&path), Some(&bundled)).unwrap(),
            fs::canonicalize(&relative).unwrap()
        );
        assert_eq!(
            resolve_from(Path::new("codex"), Some(&path), Some(&bundled)).unwrap(),
            fs::canonicalize(&path_codex).unwrap()
        );
        assert_eq!(
            resolve_from(Path::new("custom-codex"), Some(&path), Some(&bundled)).unwrap(),
            fs::canonicalize(&competing).unwrap()
        );
        fs::remove_file(&explicit).unwrap();
        assert!(matches!(
            resolve_from(&explicit, Some(&path), Some(&bundled)),
            Err(Error::ResolveExecutable { program, .. }) if program == explicit
        ));
        fs::remove_file(&path_codex).unwrap();
        assert_eq!(
            resolve_from(Path::new("codex"), Some(&path), Some(&bundled)).unwrap(),
            fs::canonicalize(&bundled).unwrap()
        );
        assert_eq!(
            resolve_from(Path::new("codex"), None, Some(&bundled)).unwrap(),
            fs::canonicalize(&bundled).unwrap()
        );
        assert!(
            matches!(resolve_from(Path::new("custom-codex"), None, Some(&bundled)), Err(Error::ExecutableNotFound(path)) if path == Path::new("custom-codex"))
        );
        fs::remove_file(&bundled).unwrap();
        for path in [Some(path.as_os_str()), None] {
            assert!(
                matches!(resolve_from(Path::new("codex"), path, Some(&bundled)), Err(Error::ExecutableNotFound(program)) if program == Path::new("codex"))
            );
        }
    }

    #[test]
    fn missing_codex_names_every_place_it_is_looked_up() {
        let message = Error::ExecutableNotFound(PathBuf::from("codex")).to_string();
        assert!(message.starts_with("Codex executable was not found: codex. "));
        assert!(message.to_lowercase().contains("put codex on path"));
        assert!(message.contains("--codex") && message.contains("BEX_CODEX"));
        if let Some(bundled) = bundled_codex_path() {
            assert!(bundled.starts_with("/Applications/ChatGPT.app"));
            assert!(message.contains("ChatGPT.app in /Applications"));
        }
    }
}
