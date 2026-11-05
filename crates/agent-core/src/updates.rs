//! Desktop release selection. Downloads and installation belong to the OS client.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    #[default]
    Production,
    Development,
}

impl Channel {
    pub fn releases_url(self) -> &'static str {
        match self {
            Self::Production => "https://api.github.com/repos/ttizze/remote-agent/releases/latest",
            Self::Development => {
                "https://api.github.com/repos/ttizze/remote-agent/releases?per_page=100"
            }
        }
    }

    fn tag_prefix(self) -> &'static str {
        match self {
            Self::Production => "prd-",
            Self::Development => "dev-",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub channel: Channel,
    pub build: u64,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Releases {
    Latest(Release),
    List(Vec<Release>),
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

pub fn select_download(
    bytes: &[u8],
    current_build: u64,
    os: &str,
    arch: &str,
    channel: Channel,
) -> Result<Option<Download>, String> {
    let file_name = match (os, arch) {
        ("macos", "aarch64") => "bex-macOS-arm64.zip",
        ("windows", "x86_64") => "bex-windows-x86_64-setup.exe",
        _ => return Err("この環境ではアプリ内更新に対応していません。".into()),
    };
    let releases: Releases = serde_json::from_slice(bytes)
        .map_err(|_| "更新情報を読み取れません。もう一度お試しください。")?;
    let releases = match releases {
        Releases::Latest(release) => vec![release],
        Releases::List(releases) => releases,
    };
    let selected = releases
        .into_iter()
        .filter(|release| !release.draft && release.prerelease == (channel == Channel::Development))
        .filter_map(|release| {
            let build = release
                .tag_name
                .strip_prefix(channel.tag_prefix())?
                .parse::<u64>()
                .ok()?;
            let asset = release
                .assets
                .into_iter()
                .find(|asset| asset.name == file_name && asset.state == "uploaded")?;
            Some((build, asset))
        })
        .max_by_key(|(build, _)| *build);
    let Some((build, asset)) = selected else {
        if channel == Channel::Development {
            return Ok(None);
        }
        return Err("この環境向けの更新ファイルはまだ公開されていません。".into());
    };
    if build <= current_build {
        return Ok(None);
    }
    let url = format!(
        "https://github.com/ttizze/remote-agent/releases/download/{}{build}/{file_name}",
        channel.tag_prefix()
    );
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"));
    let sha256 = digest
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("更新ファイルの検証情報がありません。時間をおいて再確認してください。")?;
    if asset.browser_download_url != url || asset.size == 0 {
        return Err("更新ファイルの配布情報が正しくありません。".into());
    }
    Ok(Some(Download {
        channel,
        build,
        file_name: file_name.into(),
        url,
        size: asset.size,
        sha256: sha256.to_ascii_lowercase(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn release(build: u64, name: &str) -> Value {
        json!({"tag_name":format!("prd-{build}"), "draft":false, "prerelease":false,
            "assets":[{"name":name,"state":"uploaded","size":42,
                "browser_download_url":format!("https://github.com/ttizze/remote-agent/releases/download/prd-{build}/{name}"),
                "digest":format!("sha256:{}", "ab".repeat(32))}]})
    }
    fn select(
        releases: Vec<Value>,
        current: u64,
        os: &str,
        arch: &str,
    ) -> Result<Option<Download>, String> {
        select_download(
            &serde_json::to_vec(&releases).unwrap(),
            current,
            os,
            arch,
            Channel::Production,
        )
    }

    #[test]
    fn channels_use_the_same_app_and_only_their_own_published_releases() {
        for (os, arch, file) in [
            ("macos", "aarch64", "bex-macOS-arm64.zip"),
            ("windows", "x86_64", "bex-windows-x86_64-setup.exe"),
        ] {
            let stable = release(95, file);
            let mut dev = release(96, file);
            dev["tag_name"] = "dev-96".into();
            dev["prerelease"] = true.into();
            dev["assets"][0]["browser_download_url"] =
                format!("https://github.com/ttizze/remote-agent/releases/download/dev-96/{file}")
                    .into();
            let mut draft = dev.clone();
            draft["draft"] = true.into();
            let bytes = serde_json::to_vec(&vec![draft, stable.clone(), dev]).unwrap();
            for (channel, build, prefix) in [
                (Channel::Production, 95, "prd"),
                (Channel::Development, 96, "dev"),
            ] {
                let download = select_download(&bytes, 94, os, arch, channel)
                    .unwrap()
                    .unwrap();
                assert_eq!(download.channel, channel);
                assert_eq!(download.file_name, file);
                assert_eq!(download.build, build);
                assert!(download.url.ends_with(&format!("/{prefix}-{build}/{file}")));
                // Switching back to stable must never downgrade a development build.
                assert_eq!(
                    select_download(&bytes, 97, os, arch, channel).unwrap(),
                    None
                );
            }
            // The stable endpoint is independent of how many dev releases exist.
            assert_eq!(
                select_download(
                    &serde_json::to_vec(&stable).unwrap(),
                    94,
                    os,
                    arch,
                    Channel::Production
                )
                .unwrap()
                .unwrap()
                .build,
                95
            );
        }
        assert_eq!(Channel::default(), Channel::Production);
        // Stable discovery must not depend on a finite page of dev releases.
        assert_eq!(
            Channel::Production.releases_url(),
            "https://api.github.com/repos/ttizze/remote-agent/releases/latest"
        );
        assert_eq!(
            Channel::Development.releases_url(),
            "https://api.github.com/repos/ttizze/remote-agent/releases?per_page=100"
        );
        assert_eq!(
            select_download(b"[]", 94, "macos", "aarch64", Channel::Development).unwrap(),
            None
        );
    }

    #[test]
    fn only_newer_uploaded_stable_builds_for_this_platform_are_offered() {
        let mac = "bex-macOS-arm64.zip";
        let win = "bex-windows-x86_64-setup.exe";
        let mut preview = release(999, mac);
        preview["prerelease"] = true.into();
        let mut draft = release(998, mac);
        draft["draft"] = true.into();
        let mut pending = release(997, mac);
        pending["assets"][0]["state"] = "new".into();
        let releases = vec![
            release(94, win),
            preview,
            draft,
            pending,
            release(95, mac),
            release(93, mac),
        ];
        assert_eq!(
            select(releases.clone(), 94, "macos", "aarch64")
                .unwrap()
                .unwrap()
                .build,
            95
        );
        assert_eq!(
            select(releases.clone(), 93, "windows", "x86_64")
                .unwrap()
                .unwrap()
                .build,
            94
        );
        for current in [95, 96] {
            assert_eq!(
                select(releases.clone(), current, "macos", "aarch64").unwrap(),
                None
            );
        }
        assert!(select(releases, 0, "linux", "x86_64").is_err());
    }

    #[test]
    fn invalid_or_unverifiable_downloads_are_never_offered() {
        let original = release(95, "bex-macOS-arm64.zip");
        for (key, value) in [
            ("browser_download_url", json!("https://example.com/app.zip")),
            ("size", json!(0)),
            ("digest", Value::Null),
            ("digest", json!("sha256:bad")),
        ] {
            let mut release = original.clone();
            release["assets"][0][key] = value;
            assert!(
                select(vec![release], 94, "macos", "aarch64").is_err(),
                "{key}"
            );
        }
        assert!(select_download(b"not json", 94, "macos", "aarch64", Channel::Production).is_err());
        assert!(select(vec![], 94, "macos", "aarch64").is_err());
    }

    proptest::proptest! {
        #[test]
        fn build_numbers_never_offer_an_equal_or_older_version(current in 1u64..10_000, published in 1u64..10_000) {
            let offered = select(vec![release(published, "bex-macOS-arm64.zip")], current, "macos", "aarch64").unwrap();
            proptest::prop_assert_eq!(offered.is_some(), published > current);
            if let Some(download) = offered { proptest::prop_assert_eq!(download.build, published); }
        }
    }
}
