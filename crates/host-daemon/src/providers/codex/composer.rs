//! Normalize Codex's catalog at the provider boundary.
use super::Codex;
use crate::providers::Failure;
use agent_protocol::{
    composer::{ComposerCandidate, ComposerCatalog, Invocation, InvocationKind},
    session::ProviderKind,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

#[derive(Deserialize)]
struct Skills {
    data: Vec<SkillGroup>,
}
#[derive(Deserialize)]
struct SkillGroup {
    skills: Vec<Skill>,
    errors: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Skill {
    name: String,
    path: String,
    description: String,
    enabled: bool,
    plugin_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plugins {
    marketplaces: Vec<Marketplace>,
    marketplace_load_errors: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Marketplace {
    plugins: Vec<Plugin>,
}
#[derive(Deserialize)]
struct Plugin {
    id: String,
    name: String,
    installed: bool,
    enabled: bool,
    availability: String,
    interface: Option<PluginInterface>,
}
impl Plugin {
    fn available(&self) -> bool {
        self.installed && self.enabled && self.availability == "AVAILABLE"
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginInterface {
    display_name: Option<String>,
    short_description: Option<String>,
}

impl Codex {
    pub(super) async fn composer_catalog(&self, cwd: &str) -> ComposerCatalog {
        let cwds: Vec<&str> = if cwd.is_empty() { vec![] } else { vec![cwd] };
        let params = json!({"cwds":cwds});
        let (skills, plugins) = tokio::join!(
            self.request::<_, Skills>("skills/list", &params),
            self.request::<_, Plugins>("plugin/list", &params)
        );
        normalize_catalog(cwd, skills, plugins)
    }
}

fn normalize_catalog(
    cwd: &str,
    skills: Result<Skills, Failure>,
    plugins: Result<Plugins, Failure>,
) -> ComposerCatalog {
    let available_plugins: HashSet<_> = plugins
        .as_ref()
        .ok()
        .into_iter()
        .flat_map(|plugins| &plugins.marketplaces)
        .flat_map(|marketplace| &marketplace.plugins)
        .filter(|plugin| plugin.available())
        .map(|plugin| plugin.id.as_str())
        .collect();
    let mut catalog = ComposerCatalog {
        cwd: cwd.into(),
        ..Default::default()
    };
    match skills {
        Ok(skills) => {
            for group in skills.data {
                if !group.errors.is_empty() {
                    catalog
                        .errors
                        .entry(ProviderKind::Codex)
                        .or_default()
                        .push("一部のスキルを読み込めませんでした".into());
                }
                catalog.candidates.extend(
                    group
                        .skills
                        .into_iter()
                        .filter(|skill| {
                            // A cached skill list can outlive its parent plugin's enablement.
                            // Unverified parents are excluded, including when plugin/list fails.
                            skill.enabled
                                && skill
                                    .plugin_id
                                    .as_deref()
                                    .is_none_or(|id| available_plugins.contains(id))
                        })
                        .map(|s| ComposerCandidate {
                            invocation: Invocation {
                                provider: ProviderKind::Codex,
                                kind: InvocationKind::Skill,
                                name: s.name,
                                path: s.path,
                            },
                            description: s.description,
                        }),
                );
            }
        }
        Err(_) => catalog
            .errors
            .entry(ProviderKind::Codex)
            .or_default()
            .push("スキルを取得できませんでした".into()),
    }
    match plugins {
        Ok(plugins) => {
            if !plugins.marketplace_load_errors.is_empty() {
                catalog
                    .errors
                    .entry(ProviderKind::Codex)
                    .or_default()
                    .push("一部のプラグインを読み込めませんでした".into());
            }
            catalog.candidates.extend(
                plugins
                    .marketplaces
                    .into_iter()
                    .flat_map(|m| m.plugins)
                    .filter(Plugin::available)
                    .map(|p| {
                        let (name, description) =
                            p.interface.map_or((p.name.clone(), String::new()), |i| {
                                (
                                    i.display_name.unwrap_or(p.name),
                                    i.short_description.unwrap_or_default(),
                                )
                            });
                        ComposerCandidate {
                            invocation: Invocation {
                                provider: ProviderKind::Codex,
                                kind: InvocationKind::Plugin,
                                name,
                                path: format!("plugin://{}", p.id),
                            },
                            description,
                        }
                    }),
            );
        }
        Err(_) => catalog
            .errors
            .entry(ProviderKind::Codex)
            .or_default()
            .push("プラグインを取得できませんでした".into()),
    }
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skills() -> Skills {
        serde_json::from_value(json!({"data":[{"skills":[
            {"name":"local","path":"/project/local/SKILL.md","description":"Local skill","enabled":true},
            {"name":"plugin:local","path":"/project/namespaced/SKILL.md","description":"Local namespaced skill","enabled":true,"pluginId":null},
            {"name":"system","path":"/system/SKILL.md","description":"System skill","enabled":true,"pluginId":null},
            {"name":"disabled-skill","path":"/project/disabled/SKILL.md","description":"Disabled skill","enabled":false},
            {"name":"plugin:active","path":"/cache/active/SKILL.md","description":"Active plugin skill","enabled":true,"pluginId":"plugin@one"},
            {"name":"plugin:disabled-child","path":"/cache/disabled-child/SKILL.md","description":"Disabled plugin skill","enabled":false,"pluginId":"plugin@one"},
            {"name":"plugin:other-marketplace","path":"/cache/other/SKILL.md","description":"Other marketplace","enabled":true,"pluginId":"plugin@two"},
            {"name":"missing:skill","path":"/cache/missing/SKILL.md","description":"Unknown parent","enabled":true,"pluginId":"missing@one"}
        ],"errors":[]}]})).unwrap()
    }

    fn plugins(installed: bool, enabled: bool, availability: &str) -> Plugins {
        serde_json::from_value(json!({
            "marketplaces":[{"plugins":[{
                "id":"plugin@one","name":"plugin","installed":installed,
                "enabled":enabled,"availability":availability,
                "interface":{"displayName":"Installed Plugin","shortDescription":"Plugin description"}
            }]}],"marketplaceLoadErrors":[]
        })).unwrap()
    }

    #[test]
    fn catalog_requires_an_available_exact_parent_and_an_enabled_skill() {
        for (installed, enabled, availability, usable) in [
            (true, true, "AVAILABLE", true),
            (false, true, "AVAILABLE", false),
            (true, false, "AVAILABLE", false),
            (true, true, "DISABLED_BY_ADMIN", false),
        ] {
            let catalog = normalize_catalog(
                "/project",
                Ok(skills()),
                Ok(plugins(installed, enabled, availability)),
            );
            let mut expected = vec!["local", "plugin:local", "system"];
            if usable {
                expected.extend(["plugin:active", "Installed Plugin"]);
                let child = &catalog.candidates[3];
                assert_eq!(child.invocation.kind, InvocationKind::Skill);
                assert_eq!(child.invocation.path, "/cache/active/SKILL.md");
                assert_eq!(child.description, "Active plugin skill");
                let parent = &catalog.candidates[4];
                assert_eq!(parent.invocation.kind, InvocationKind::Plugin);
                assert_eq!(parent.invocation.path, "plugin://plugin@one");
                assert_eq!(parent.description, "Plugin description");
            }
            assert_eq!(catalog.cwd, "/project");
            assert_eq!(
                catalog
                    .candidates
                    .iter()
                    .map(|c| c.invocation.name.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(catalog.errors.is_empty());
        }
    }

    #[test]
    fn failed_plugin_lookup_preserves_standalone_skills_and_recovers() {
        let catalog = normalize_catalog(
            "/project",
            Ok(skills()),
            Err(Failure::new("fixture", "unavailable")),
        );
        assert_eq!(
            catalog
                .candidates
                .iter()
                .map(|c| c.invocation.name.as_str())
                .collect::<Vec<_>>(),
            ["local", "plugin:local", "system"]
        );
        assert_eq!(
            catalog.errors[&ProviderKind::Codex],
            ["プラグインを取得できませんでした"]
        );

        let recovered = normalize_catalog(
            "/project",
            Ok(skills()),
            Ok(plugins(true, true, "AVAILABLE")),
        );
        assert!(
            recovered
                .candidates
                .iter()
                .any(|c| c.invocation.name == "plugin:active")
        );
        assert!(recovered.errors.is_empty());
    }

    #[test]
    fn failed_skill_lookup_preserves_available_plugin_mentions() {
        let catalog = normalize_catalog(
            "/project",
            Err(Failure::new("fixture", "unavailable")),
            Ok(plugins(true, true, "AVAILABLE")),
        );
        assert_eq!(catalog.candidates.len(), 1);
        assert_eq!(catalog.candidates[0].invocation.name, "Installed Plugin");
        assert_eq!(
            catalog.errors[&ProviderKind::Codex],
            ["スキルを取得できませんでした"]
        );
    }

    #[test]
    fn partial_catalog_errors_keep_verified_candidates_and_report_both_sources() {
        let mut skills = skills();
        skills.data[0]
            .errors
            .push(json!({"message":"skill load failed"}));
        let mut plugins = plugins(true, true, "AVAILABLE");
        plugins
            .marketplace_load_errors
            .push(json!({"message":"marketplace load failed"}));
        let catalog = normalize_catalog("/project", Ok(skills), Ok(plugins));
        assert_eq!(catalog.candidates.len(), 5);
        assert_eq!(
            catalog.errors[&ProviderKind::Codex],
            [
                "一部のスキルを読み込めませんでした",
                "一部のプラグインを読み込めませんでした",
            ]
        );
    }
}
