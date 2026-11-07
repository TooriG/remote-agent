use agent_protocol::browser::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedTab {
    pub(crate) scope: String,
    pub(crate) id: String,
    pub(crate) url: String,
    pub(crate) opener: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Page {
    pub(crate) active: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) generation: u64,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            active: String::new(),
            width: WIDTH,
            height: HEIGHT,
            generation: 0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Saved {
    pub(crate) profile: String,
    pub(crate) pages: HashMap<String, Page>,
    pub(crate) tabs: Vec<SavedTab>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            profile: uuid::Uuid::new_v4().to_string(),
            pages: HashMap::new(),
            tabs: Vec::new(),
        }
    }
}
impl Saved {
    pub(crate) fn validate(&self) -> Result<(), String> {
        use std::collections::HashSet;
        let profile = uuid::Uuid::parse_str(&self.profile)
            .map_err(|_| "ブラウザのプロファイルが不正です。")?;
        if profile.is_nil() || self.pages.len() > 32 || self.tabs.len() > 32 * 24 {
            return Err("ブラウザ保存データが範囲外です。".into());
        }
        let mut ids = HashSet::new();
        for tab in &self.tabs {
            if tab.id.is_empty()
                || tab.id.len() > 256
                || !ids.insert(&tab.id)
                || !self.pages.contains_key(&tab.scope)
            {
                return Err("保存したタブの所属が不正です。".into());
            }
            browser_url(&tab.url)?;
        }
        for (scope, page) in &self.pages {
            BrowserAction::Resize {
                width: page.width,
                height: page.height,
            }
            .validate()?;
            let tabs: Vec<_> = self.tabs.iter().filter(|t| &t.scope == scope).collect();
            if scope.is_empty()
                || scope.len() > 8192
                || tabs.len() > 24
                || (!page.active.is_empty() && !tabs.iter().any(|t| t.id == page.active))
                || (page.active.is_empty() && !tabs.is_empty())
            {
                return Err("保存した会話のブラウザが不正です。".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restored_tabs_have_unique_ids_and_belong_to_their_saved_conversation() {
        let mut saved = Saved::default();
        assert!(saved.validate().is_ok());
        saved.pages.insert(
            "a".into(),
            Page {
                active: "tab-a".into(),
                ..Default::default()
            },
        );
        saved.tabs.push(SavedTab {
            scope: "a".into(),
            id: "tab-a".into(),
            url: "https://example.test/".into(),
            opener: None,
        });
        assert!(saved.validate().is_ok());
        let mut invalid = saved.clone();
        invalid.profile = uuid::Uuid::nil().to_string();
        assert!(invalid.validate().is_err());
        let mut invalid = saved.clone();
        invalid.tabs[0].scope = "b".into();
        assert!(invalid.validate().is_err());
        let mut invalid = saved.clone();
        invalid.tabs.push(invalid.tabs[0].clone());
        assert!(invalid.validate().is_err());
        let mut invalid = saved.clone();
        invalid.pages.get_mut("a").unwrap().active = "tab-b".into();
        assert!(invalid.validate().is_err());
        let mut invalid = saved.clone();
        invalid.pages.get_mut("a").unwrap().width = 0;
        assert!(invalid.validate().is_err());
        saved.tabs[0].url = "file:///private/data".into();
        assert!(saved.validate().is_err());
    }
}
