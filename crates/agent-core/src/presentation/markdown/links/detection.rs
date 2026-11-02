//! Recognition follows t3code 8b74a690's shared/markdownLinks and mobile's actual
//! enriched Markdown renderer. The latter deliberately has a different code-span grammar.
use std::sync::LazyLock;

use regex::Regex;

use super::paths;

static RELATIVE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[\p{L}\p{M}\p{N}._-]+(?: +[\p{L}\p{M}\p{N}._-]+)*/)+(?:[\p{L}\p{M}\p{N}._-]+(?: +[\p{L}\p{M}\p{N}._-]+)*(?::[0-9]+){0,2})?$").unwrap()
});
static RELATIVE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\p{L}\p{M}\p{N}._-]+(?: +[\p{L}\p{M}\p{N}._-]+)*\.[\p{L}\p{M}\p{N}_-]+(?::[0-9]+){0,2}$").unwrap()
});
static SCHEME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:(.*)$").unwrap());
static POSITION_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?::[0-9]+)?$").unwrap());
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z0-9_-]+$").unwrap());
static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]\.[0-9][^.]*$").unwrap());
static NUMERIC_DOTTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]+)+$").unwrap());
static MOBILE_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:\.{1,2}[\\/]|~[\\/]|/(?:Users|home|tmp|var|etc|opt|mnt|Volumes|private|root|usr|bin|sbin|lib|lib64|srv|dev|proc|sys|run|boot|media|workspace|workspaces)/|[A-Za-z]:[\\/])[^\s`]+|(?:[A-Za-z0-9._-]+[\\/])+[A-Za-z0-9._-]+\.[A-Za-z0-9_-]+(?::[0-9]+){0,2}|[A-Za-z0-9._-]+\.[A-Za-z0-9_-]+:[0-9]+(?::[0-9]+)?)$").unwrap()
});

const ROOTS: &[&str] = &[
    "/Users/",
    "/home/",
    "/tmp/",
    "/var/",
    "/etc/",
    "/opt/",
    "/mnt/",
    "/Volumes/",
    "/private/",
    "/root/",
    "/usr/",
    "/bin/",
    "/sbin/",
    "/lib/",
    "/lib64/",
    "/srv/",
    "/dev/",
    "/proc/",
    "/sys/",
    "/run/",
    "/boot/",
    "/media/",
    "/workspace/",
    "/workspaces/",
];
const EXTENSIONLESS: &[&str] = &[
    "Makefile",
    "makefile",
    "GNUmakefile",
    "Dockerfile",
    "Containerfile",
    "Justfile",
    "justfile",
    "Rakefile",
    "Gemfile",
    "Procfile",
    "Brewfile",
    "Caddyfile",
    "Vagrantfile",
    "Jenkinsfile",
    "Podfile",
    "Fastfile",
    "BUILD",
    "WORKSPACE",
    "LICENSE",
    "LICENCE",
    "COPYING",
    "NOTICE",
    "AUTHORS",
    "CONTRIBUTORS",
    "CHANGELOG",
    "README",
    "CODEOWNERS",
];
const GENERIC_TLDS: &[&str] = &[
    "com", "net", "org", "io", "dev", "app", "ai", "co", "edu", "gov", "mil", "info", "biz", "xyz",
    "me", "tv", "cc", "gg", "chat", "cloud", "site", "online", "tech", "store", "link",
];
const COUNTRY_TLDS: &[&str] = &[
    "uk", "de", "fr", "nl", "se", "no", "fi", "dk", "pl", "ch", "at", "be", "es", "it", "pt", "eu",
    "us", "ca", "au", "nz", "jp", "kr", "cn", "br", "ru", "mx", "ie", "cz", "tr", "sg", "hk",
];

#[derive(Clone, Copy)]
pub(crate) enum Syntax {
    Link,
    DesktopCode,
    MobileCode,
}

pub(super) fn explicit_relative(path: &str) -> bool {
    ["~/", "./", "../"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn host_name(segment: &str, positioned: bool) -> bool {
    if segment.starts_with('.') {
        return false;
    }
    let segment = segment.to_ascii_lowercase();
    if segment == "localhost" || NUMERIC_DOTTED.is_match(&segment) {
        return true;
    }
    segment.rsplit_once('.').is_some_and(|(_, tld)| {
        GENERIC_TLDS.contains(&tld) || (!positioned && COUNTRY_TLDS.contains(&tld))
    })
}

pub(super) fn candidate(source: &str, syntax: Syntax) -> Option<String> {
    match syntax {
        Syntax::Link => Some(source.trim().into()),
        Syntax::MobileCode => {
            (!source.contains("://") && MOBILE_CODE.is_match(source)).then(|| source.into())
        }
        Syntax::DesktopCode => desktop_code(source.trim()),
    }
}

fn desktop_code(source: &str) -> Option<String> {
    if source.is_empty()
        || source
            .chars()
            .any(|char| char.is_whitespace() || char == '`')
    {
        return None;
    }
    let candidate = if paths::windows(source) {
        source.into()
    } else {
        source.replace('\\', "/")
    };
    let (without_position, _, _) = paths::position(&candidate, "");
    // Zero positions still count as a positional shape in T3's candidate check.
    let positioned = without_position.len() != candidate.len();
    if !positioned && !candidate.contains(['/', '\\']) {
        return None;
    }
    if explicit_relative(&candidate) || paths::absolute(&candidate) {
        return Some(candidate);
    }
    let basename = paths::basename(without_position);
    if without_position.ends_with('/')
        || host_name(
            without_position.split('/').next().unwrap_or_default(),
            positioned,
        )
        || VERSION.is_match(basename)
        || (!positioned && !EXTENSION.is_match(basename))
    {
        return None;
    }
    Some(candidate)
}

pub(super) fn file_shape(path: &str, authored_path: &str) -> bool {
    if paths::windows(path) || explicit_relative(path) {
        return true;
    }
    if path.starts_with('/') {
        return ROOTS.iter().any(|root| authored_path.starts_with(root))
            || paths::position(authored_path, "").0.len() != authored_path.len()
            || EXTENSIONLESS.contains(&paths::basename(authored_path))
            || EXTENSION.is_match(paths::basename(authored_path));
    }
    EXTENSIONLESS.contains(&path)
        || RELATIVE_PATH.is_match(authored_path)
        || RELATIVE_NAME.is_match(path)
}

pub(super) fn external_scheme(path: &str) -> bool {
    !paths::windows(path)
        && SCHEME.captures(path).is_some_and(|captures| {
            let rest = captures.get(1).unwrap().as_str();
            rest.starts_with("//") || !POSITION_ONLY.is_match(rest)
        })
}
