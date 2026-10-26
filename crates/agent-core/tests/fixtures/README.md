# Fixture provenance

The inherited behavior corpus comes from `ttizze/remote-agent` commit `ea8aefc`,
`crates/agent-client/tests/fixtures/`. Command tags identify scenarios; the harness
uses current typed operations rather than treating those tags as a wire API.

`daemon-wire.json` originates from an isolated Host and `bex-codex-fixture`, not
production data. Only the temporary workspace path is normalized.

Markdown and visualization fixtures are shared source inputs for Rust, Swift and
Android rendering tests; preserve reported input cases when changing assertions.
