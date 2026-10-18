//! Credential references understood by the DSH provider routes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyProvider {
    #[default]
    DeepSeek,
    OpenRouter,
    OpenAI,
    Anthropic,
    Gemini,
    Groq,
    Mistral,
    Xai,
}

impl KeyProvider {
    /// DSH advertises opaque JSON [provider, model] values over ACP.
    pub fn for_model_id(id: &str) -> Option<Self> {
        let [route, model]: [String; 2] = serde_json::from_str(id).ok()?;
        if model.is_empty() { return None; }
        Self::ALL.into_iter().find(|provider| provider.route() == route)
    }

    pub fn model_label(self) -> &'static str {
        if self == Self::DeepSeek { "DeepSeek公式" } else { self.label() }
    }

    pub const ALL: [Self; 8] = [
        Self::DeepSeek,
        Self::OpenRouter,
        Self::OpenAI,
        Self::Anthropic,
        Self::Gemini,
        Self::Groq,
        Self::Mistral,
        Self::Xai,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenRouter => "OpenRouter",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::Groq => "Groq",
            Self::Mistral => "Mistral",
            Self::Xai => "xAI",
        }
    }

    pub fn reference(self) -> &'static str {
        match self {
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::OpenAI => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Mistral => "MISTRAL_API_KEY",
            Self::Xai => "XAI_API_KEY",
        }
    }

    pub fn route(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek-official",
            Self::OpenRouter => "openrouter",
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "google",
            Self::Groq => "groq",
            Self::Mistral => "mistral",
            Self::Xai => "xai",
        }
    }

    pub fn key_url(self) -> &'static str {
        match self {
            Self::DeepSeek => "https://platform.deepseek.com/api_keys",
            Self::OpenRouter => "https://openrouter.ai/settings/keys",
            Self::OpenAI => "https://platform.openai.com/api-keys",
            Self::Anthropic => "https://console.anthropic.com/settings/keys",
            Self::Gemini => "https://aistudio.google.com/apikey",
            Self::Groq => "https://console.groq.com/keys",
            Self::Mistral => "https://console.mistral.ai/api-keys",
            Self::Xai => "https://console.x.ai/",
        }
    }
}

/// Carried by the existing private account-login submission. Never returned in metadata.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyUpdate {
    pub provider: KeyProvider,
    /// None removes only the selected provider's BEX-managed key.
    pub key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_routes_preserve_provider_boundaries() {
        for provider in KeyProvider::ALL {
            let id = serde_json::to_string(&[provider.route(), "test/model"]).unwrap();
            assert!(KeyProvider::for_model_id(&id) == Some(provider));
        }
        for id in ["deepseek-flash", "[]", r#"["openrouter",""]"#, r#"["unknown","model"]"#] {
            assert!(KeyProvider::for_model_id(id).is_none());
        }
    }
}
