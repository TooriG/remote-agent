//! Mobile glyphs from T3 Code apps/mobile/src/lib/markdownLinks.ts.
use regex::Regex;
use std::sync::LazyLock;
static POSITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":[0-9]+(?::[0-9]+)?$").unwrap());

/// The mobile catalog intentionally differs from the desktop Pierre resolver.
pub(super) fn icon(path: &str) -> &'static str {
    let basename = super::super::markdown::file_basename(path);
    let name = POSITION.replace(basename, "").to_lowercase();
    if name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension,
            "avi" | "m4v" | "mkv" | "mov" | "mp4" | "ogv" | "webm"
        )
    }) {
        return "video";
    }
    if let Some(icon) = lookup(NAMES, &name) {
        return icon;
    }
    if name.starts_with("tsconfig.") && name.ends_with(".json") {
        return "typescript";
    }
    name.match_indices('.')
        .find_map(|(index, _)| lookup(EXTENSIONS, &name[index + 1..]))
        .unwrap_or("default")
}
fn lookup(rules: &[(&str, &'static str)], name: &str) -> Option<&'static str> {
    rules
        .binary_search_by_key(&name, |(key, _)| key)
        .ok()
        .map(|index| rules[index].1)
}

const NAMES: &[(&str, &str)] = &[
    (".babelrc", "babel"),
    (".babelrc.json", "babel"),
    (".bash_profile", "bash"),
    (".bashrc", "bash"),
    (".browserslistrc", "browserslist"),
    (".dockerignore", "docker"),
    (".eslintignore", "eslint"),
    (".eslintrc", "eslint"),
    (".eslintrc.cjs", "eslint"),
    (".eslintrc.js", "eslint"),
    (".eslintrc.json", "eslint"),
    (".eslintrc.yaml", "eslint"),
    (".eslintrc.yml", "eslint"),
    (".gitattributes", "git"),
    (".gitignore", "git"),
    (".gitkeep", "git"),
    (".gitmodules", "git"),
    (".oxlintrc.json", "oxc"),
    (".postcssrc", "postcss"),
    (".postcssrc.json", "postcss"),
    (".postcssrc.yaml", "postcss"),
    (".postcssrc.yml", "postcss"),
    (".prettierignore", "prettier"),
    (".prettierrc", "prettier"),
    (".prettierrc.cjs", "prettier"),
    (".prettierrc.js", "prettier"),
    (".prettierrc.json", "prettier"),
    (".prettierrc.mjs", "prettier"),
    (".prettierrc.toml", "prettier"),
    (".prettierrc.yaml", "prettier"),
    (".prettierrc.yml", "prettier"),
    (".stylelintignore", "stylelint"),
    (".stylelintrc", "stylelint"),
    (".stylelintrc.cjs", "stylelint"),
    (".stylelintrc.js", "stylelint"),
    (".stylelintrc.json", "stylelint"),
    (".stylelintrc.mjs", "stylelint"),
    (".stylelintrc.yaml", "stylelint"),
    (".stylelintrc.yml", "stylelint"),
    (".terraform.lock.hcl", "terraform"),
    (".zprofile", "bash"),
    (".zshenv", "bash"),
    (".zshrc", "bash"),
    ("agents.md", "agents"),
    ("babel.config.cjs", "babel"),
    ("babel.config.js", "babel"),
    ("babel.config.json", "babel"),
    ("babel.config.mjs", "babel"),
    ("biome.json", "biome"),
    ("biome.jsonc", "biome"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("bunfig.toml", "bun"),
    ("claude.md", "claude"),
    ("compose.yaml", "docker"),
    ("compose.yml", "docker"),
    ("docker-compose.override.yml", "docker"),
    ("docker-compose.yaml", "docker"),
    ("docker-compose.yml", "docker"),
    ("dockerfile", "docker"),
    ("eslint.config.cjs", "eslint"),
    ("eslint.config.js", "eslint"),
    ("eslint.config.mjs", "eslint"),
    ("eslint.config.mts", "eslint"),
    ("eslint.config.ts", "eslint"),
    ("gemfile", "ruby"),
    ("next.config.js", "nextjs"),
    ("next.config.mjs", "nextjs"),
    ("next.config.mts", "nextjs"),
    ("next.config.ts", "nextjs"),
    ("package.json", "npm"),
    ("pnpm-lock.yaml", "pnpm"),
    ("pnpm-workspace.yaml", "pnpm"),
    ("postcss.config.cjs", "postcss"),
    ("postcss.config.js", "postcss"),
    ("postcss.config.mjs", "postcss"),
    ("postcss.config.ts", "postcss"),
    ("prettier.config.cjs", "prettier"),
    ("prettier.config.js", "prettier"),
    ("prettier.config.mjs", "prettier"),
    ("rakefile", "ruby"),
    ("readme.md", "markdown"),
    ("stylelint.config.cjs", "stylelint"),
    ("stylelint.config.js", "stylelint"),
    ("stylelint.config.mjs", "stylelint"),
    ("svgo.config.cjs", "svgo"),
    ("svgo.config.js", "svgo"),
    ("svgo.config.mjs", "svgo"),
    ("svgo.config.ts", "svgo"),
    ("tailwind.config.cjs", "tailwind"),
    ("tailwind.config.js", "tailwind"),
    ("tailwind.config.mjs", "tailwind"),
    ("tailwind.config.ts", "tailwind"),
    ("tsconfig.json", "typescript"),
    ("vite.config.js", "vite"),
    ("vite.config.mjs", "vite"),
    ("vite.config.mts", "vite"),
    ("vite.config.ts", "vite"),
    ("webpack.config.babel.js", "webpack"),
    ("webpack.config.cjs", "webpack"),
    ("webpack.config.js", "webpack"),
    ("webpack.config.mjs", "webpack"),
    ("webpack.config.ts", "webpack"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("7z", "zip"),
    ("astro", "astro"),
    ("avif", "image"),
    ("bash", "bash"),
    ("bmp", "image"),
    ("bz2", "zip"),
    ("c", "c"),
    ("cc", "cpp"),
    ("code-workspace", "vscode"),
    ("cpp", "cpp"),
    ("css", "css"),
    ("csv", "table"),
    ("cts", "typescript"),
    ("cxx", "cpp"),
    ("db", "database"),
    ("env", "text"),
    ("env.development", "text"),
    ("env.local", "text"),
    ("env.production", "text"),
    ("eot", "font"),
    ("erb", "ruby"),
    ("fish", "bash"),
    ("gif", "image"),
    ("go", "go"),
    ("gql", "graphql"),
    ("graphql", "graphql"),
    ("gz", "zip"),
    ("h", "c"),
    ("hh", "cpp"),
    ("hpp", "cpp"),
    ("htm", "html"),
    ("html", "html"),
    ("hxx", "cpp"),
    ("icns", "image"),
    ("ico", "image"),
    ("ini", "text"),
    ("inl", "cpp"),
    ("jar", "zip"),
    ("jpeg", "image"),
    ("jpg", "image"),
    ("js", "javascript"),
    ("json", "json"),
    ("jsonc", "json"),
    ("jsx", "react"),
    ("less", "css"),
    ("md", "markdown"),
    ("mdx", "markdown"),
    ("mdx.tsx", "markdown"),
    ("mjs", "javascript"),
    ("mts", "typescript"),
    ("png", "image"),
    ("postcss", "css"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("pyx", "python"),
    ("rake", "ruby"),
    ("rar", "zip"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("sass", "sass"),
    ("scss", "sass"),
    ("sh", "bash"),
    ("sql", "database"),
    ("sqlite", "database"),
    ("sqlite3", "database"),
    ("svelte", "svelte"),
    ("svg", "svg"),
    ("swift", "swift"),
    ("tar", "zip"),
    ("tf", "terraform"),
    ("tfstate", "terraform"),
    ("tfvars", "terraform"),
    ("tgz", "zip"),
    ("ts", "typescript"),
    ("tsv", "table"),
    ("tsx", "react"),
    ("txt", "text"),
    ("vue", "vue"),
    ("wasm", "wasm"),
    ("webp", "image"),
    ("woff", "font"),
    ("woff2", "font"),
    ("yaml", "yml"),
    ("yml", "yml"),
    ("zig", "zig"),
    ("zip", "zip"),
    ("zsh", "bash"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mobile_names_suffixes_positions_and_unsupported_formats_follow_the_mobile_catalog() {
        for (path, expected) in [
            ("SRC/Main.RS:12:3", "rust"),
            ("AGENTS.md", "agents"),
            ("tsconfig.build.json", "typescript"),
            ("index.mdx.tsx", "markdown"),
            ("sample.env.local", "text"),
            ("clip.MKV:0", "video"),
            ("icon.icns", "image"),
            ("data.csv", "table"),
            ("manual.pdf", "default"),
            ("notes.docx", "default"),
            ("Main.java", "default"),
            ("folder/unknown.xyz", "default"),
        ] {
            assert_eq!(icon(path), expected, "{path}");
        }
    }
    #[test]
    fn catalogs_are_unique_and_sorted_for_lookup() {
        for rules in [NAMES, EXTENSIONS] {
            assert!(rules.windows(2).all(|pair| pair[0].0 < pair[1].0));
        }
    }
}
