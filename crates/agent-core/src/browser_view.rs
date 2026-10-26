//! Shared address-bar decisions. Automation navigation accepts URLs, never search terms.
pub fn address_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 8192 {
        return Err("URL または検索語を入力してください。".into());
    }
    if input == "about:blank" || input.contains("://") {
        return agent_protocol::browser::browser_url(input);
    }
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    let bare_port = authority
        .rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
    if authority.contains(':') && !bare_port && !authority.starts_with('[') {
        return Err("http または https の URL を入力してください。".into());
    }
    if !input.chars().any(char::is_whitespace)
        && (authority.contains('.')
            || authority == "localhost"
            || authority.starts_with('[')
            || bare_port)
    {
        return agent_protocol::browser::browser_url(input);
    }
    let mut url = url::Url::parse("https://duckduckgo.com/").unwrap();
    url.query_pairs_mut().append_pair("q", input);
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_search_and_host_local_development_are_distinct() {
        for (input, expected) in [
            ("example.com", "https://example.com/"),
            ("localhost:3000/path", "http://localhost:3000/path"),
            ("[::1]:4000", "http://[::1]:4000/"),
            (
                "日本語 search",
                "https://duckduckgo.com/?q=%E6%97%A5%E6%9C%AC%E8%AA%9E+search",
            ),
            (
                "query&secret=other",
                "https://duckduckgo.com/?q=query%26secret%3Dother",
            ),
        ] {
            assert_eq!(address_url(input).unwrap(), expected);
        }
        for input in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:pass@example.com",
        ] {
            assert!(address_url(input).is_err());
        }
    }
}
