//! Windows folder selections identify both a Linux path and its owning distro.
//! Keep these paths out of Windows Git and do not resolve them in another distro.
use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslPath {
    pub distro: String,
    pub linux_path: String,
}

pub fn to_unc(distro: &str, linux_path: &str) -> Result<String> {
    validate_distro(distro)?;
    if !linux_path.starts_with('/') || linux_path.contains('\\') {
        bail!("WSL folder requires a Linux absolute path");
    }
    let path = format!(r"\\wsl.localhost\{distro}{}", linux_path.replace('/', "\\"));
    parse_unc(&path)?;
    Ok(path)
}

pub fn parse_unc(input: &str) -> Result<Option<WslPath>> {
    let path = input.trim().replace('\\', "/");
    let lower = path.to_ascii_lowercase();
    let Some(prefix) = [
        "//wsl.localhost/",
        "//wsl$/",
        "//?/UNC/wsl.localhost/",
        "//?/UNC/wsl$/",
    ]
    .into_iter()
    .find(|prefix| lower.starts_with(&prefix.to_ascii_lowercase())) else {
        return Ok(None);
    };
    let (distro, rest) = path[prefix.len()..]
        .split_once('/')
        .unwrap_or((&path[prefix.len()..], ""));
    validate_distro(distro)?;
    if rest.chars().any(char::is_control) || rest.split('/').any(|part| matches!(part, "." | ".."))
    {
        bail!("WSL folder requires an absolute path without parent components");
    }
    Ok(Some(WslPath {
        distro: distro.into(),
        linux_path: format!("/{}", rest.trim_start_matches('/')),
    }))
}

pub fn validate_distro(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._- ".contains(c))
        || name.ends_with(' ')
    {
        bail!("Invalid WSL distribution name");
    }
    Ok(())
}

/// wsl.exe emits UTF-16LE even when its stdout is redirected to a pipe.
pub fn parse_distros(bytes: &[u8]) -> Result<Vec<String>> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.contains(&0) {
        if !bytes.len().is_multiple_of(2) {
            bail!("Invalid WSL distribution list");
        }
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )?
    } else {
        String::from_utf8(bytes.to_vec())?
    };
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            validate_distro(name)?;
            Ok(name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_selection_preserves_owning_distro_and_linux_filename() {
        for input in [
            r"\\wsl.localhost\Ubuntu\home\user\my repo",
            r"\\wsl$\Ubuntu\home\user\my repo",
            "//WSL.LOCALHOST/Ubuntu/home/user/my repo",
            r"\\?\UNC\wsl.localhost\Ubuntu\home\user\my repo",
        ] {
            assert_eq!(
                parse_unc(input).unwrap(),
                Some(WslPath {
                    distro: "Ubuntu".into(),
                    linux_path: "/home/user/my repo".into()
                })
            );
        }
        assert_eq!(
            parse_unc(r"\\wsl$\Debian").unwrap().unwrap().linux_path,
            "/"
        );
        for input in [r"C:\repo", "/home/user/repo", r"\\server\share\repo"] {
            assert_eq!(parse_unc(input).unwrap(), None);
        }
        for input in [
            r"\\wsl$\Ubuntu$HOME\repo",
            r"\\wsl$\Ubuntu\..\repo",
            "//wsl$/Ubuntu/home/\nrepo",
            "//wsl$//home",
        ] {
            assert!(parse_unc(input).is_err(), "{input}");
        }
    }

    #[test]
    fn distribution_discovery_accepts_pipe_encodings_and_rejects_corruption() {
        let text = "\u{feff}Ubuntu\r\nDebian\r\nMy Distro\r\n";
        let utf16: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            parse_distros(&utf16).unwrap(),
            ["Ubuntu", "Debian", "My Distro"]
        );
        let without_bom: Vec<_> = "Ubuntu\r\nDebian\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(parse_distros(&without_bom).unwrap(), ["Ubuntu", "Debian"]);
        assert_eq!(
            parse_distros(b"Ubuntu\nDebian\n").unwrap(),
            ["Ubuntu", "Debian"]
        );
        assert!(parse_distros(&[0xff, 0xfe, 1]).is_err());
        assert!(parse_distros(b"--exec\n").is_err());
        assert!(parse_distros(b"Ubuntu$HOME\n").is_err());
        assert!(parse_distros(b"Ubuntu\nBad\tName\n").is_err());
    }

    #[test]
    fn folder_picker_starts_in_the_selected_wsl_environment() {
        assert_eq!(
            to_unc("Ubuntu", "/home/user/my repo").unwrap(),
            r"\\wsl.localhost\Ubuntu\home\user\my repo"
        );
        assert_eq!(
            parse_unc(&to_unc("Debian", "/").unwrap()).unwrap().unwrap(),
            WslPath {
                distro: "Debian".into(),
                linux_path: "/".into()
            }
        );
        for path in [
            "home/user",
            r"C:\repo",
            "/home/../user",
            "/home/\nrepo",
            r"/home/user\repo",
        ] {
            assert!(to_unc("Ubuntu", path).is_err(), "{path}");
        }
        assert!(to_unc("--exec", "/home").is_err());
    }
}
