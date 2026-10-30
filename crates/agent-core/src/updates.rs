//! Desktop release selection. Downloads and installation belong to the OS client.
use serde::Deserialize;

pub const RELEASES_URL: &str =
    "https://api.github.com/repos/ttizze/remote-agent/releases?per_page=100";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
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
) -> Result<Option<Download>, String> {
    let file_name = match (os, arch) {
        ("macos", "aarch64") => "bex-macOS-arm64.zip",
        ("windows", "x86_64") => "bex-windows-x86_64-setup.exe",
        _ => return Err("この環境ではアプリ内更新に対応していません。".into()),
    };
    let releases: Vec<Release> = serde_json::from_slice(bytes)
        .map_err(|_| "更新情報を読み取れません。もう一度お試しください。")?;
    // Apple and Windows assets can be published at different times. Compare
    // build numbers, independently of API order, and ignore incomplete uploads.
    let selected = releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let build = release.tag_name.strip_prefix("prd-")?.parse::<u64>().ok()?;
            let asset = release
                .assets
                .into_iter()
                .find(|asset| asset.name == file_name && asset.state == "uploaded")?;
            Some((build, asset))
        })
        .max_by_key(|(build, _)| *build);
    let Some((build, asset)) = selected else {
        return Err("この環境向けの更新ファイルはまだ公開されていません。".into());
    };
    if build <= current_build {
        return Ok(None);
    }
    let url =
        format!("https://github.com/ttizze/remote-agent/releases/download/prd-{build}/{file_name}");
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
        select_download(&serde_json::to_vec(&releases).unwrap(), current, os, arch)
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
        assert!(select_download(b"not json", 94, "macos", "aarch64").is_err());
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
