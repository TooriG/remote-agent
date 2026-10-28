Lucide 1.63.0, ISC license, vendored for offline rendering.
Source: https://registry.npmjs.org/lucide/-/lucide-1.63.0.tgz
Archive SHA256: 543354b801fcae7181a80a17a108df96e050571bb8f874c3d713489f7a18b89e
The complete upstream UMD bundle supplies the same icon API on both mobile platforms.

Reproduce these vendored files with the pinned npm lockfile:

```sh
nix develop . --command npm --prefix crates/host-daemon/src/visualize ci --ignore-scripts --no-audit
nix develop . --command npm --prefix crates/host-daemon/src/visualize run vendor
```

## Mobile rendering contract

Core recognizes standalone visualize references through its Markdown parser;
code spans, malformed references and ordinary Markdown retain their meaning.
Relative paths resolve against the conversation's Host working directory and are
read through the authenticated file service. The Host archives the latest
successfully read HTML fragment so reopening can survive source deletion and
Host restart. A never-read deleted file cannot be recovered. Selection resets
when reopening a fragment.

Both native WebViews render the same offline sandbox document. It permits
scripts but no same-origin privilege, external connections, file access, forms,
popups, top navigation or native bridge. Embedded data images/fonts remain
available. Unsupported APIs and external resources receive a visible compatibility
notice; they never dispatch native operations. iOS uses nonpersistent WebView
storage; Android blocks external navigation and gives the WebView a nonzero
viewport inside its Compose frame.

The path and sandbox rules live in [visualize.rs](../visualize.rs); authenticated
archive recovery and native interaction tests use the shared
[fragment fixture](../../../agent-core/tests/fixtures/visualize/icon-options.html).
