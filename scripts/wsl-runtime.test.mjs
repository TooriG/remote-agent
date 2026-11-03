import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const bootstrap = resolve("crates/agent-local-host/src/wsl-bootstrap.sh");

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "bex-wsl-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, "Windows install $HOME ' path");
  const tools = join(root, "tools");
  mkdirSync(join(source, "lib"), { recursive: true });
  mkdirSync(tools);
  const script = (path, contents) => writeFileSync(path, `#!/bin/sh\nset -eu\n${contents}\n`, { mode: 0o700 });
  script(join(tools, "wslpath"), 'printf "%s\\n" "$2"');
  script(join(tools, "hostname"), 'printf "172.25.10.2 172.26.1.1\\n"');
  script(join(tools, "uname"), 'printf "x86_64\\n"');
  script(join(source, "lib", "ld-linux-x86-64.so.2"), 'shift 2; exec "$@"');
  script(join(source, "patchelf"), 'printf "%s\\n" "$*" >> "$TEST_ROOT/patches"');
  script(join(source, "agent-provider-supervisor"), 'exit 0');
  script(join(source, "host-daemon"), `
original_args=$*
if [ "$1" = --state-dir ]; then shift 2; [ "$1" = --isolated ]; shift; fi
if [ "$1" = local-client ]; then
  [ -f "$TEST_ROOT/running" ] || exit 1
  case "$*" in *--require-current-binary*) [ "$(cat "$TEST_ROOT/image" 2>/dev/null)" = "$0" ] || exit 42;; esac
  printf '{"ticket":"test-ticket","address":"%s"}\\n' "$3"
elif [ "$1" = prepare-update ]; then
  : > "$TEST_ROOT/prepared"
  rm "$TEST_ROOT/running"
  printf '%s\\n' '{"executable":"old-host","arguments":["--name","Preserved WSL"],"working_directory":"/existing/work"}'
elif [ "$1" = resume-host ]; then
  cp "$3" "$TEST_ROOT/resumed-launch"
  printf '%s\\n' "$0" > "$TEST_ROOT/image"
  : > "$TEST_ROOT/running"
  printf '%s\\n' 'resumed' >> "$TEST_ROOT/starts"
else
  printf '%s\\n' "$original_args" >> "$TEST_ROOT/starts"
  printf '%s\\n' "$0" > "$TEST_ROOT/image"
  : > "$TEST_ROOT/running"
fi`);
  writeFileSync(join(source, "revision"), "a".repeat(40));
  const env = { ...process.env, PATH: `${tools}:${process.env.PATH}`, TEST_ROOT: root,
    XDG_CACHE_HOME: join(root, "linux cache"), WSL_DISTRO_NAME: "Ubuntu" };
  return { root, source, env, run: () => spawnSync("sh", [bootstrap, source], { env, encoding: "utf8", timeout: 10000 }) };
}

test("WSL bootstrap installs into Linux, starts one Host and returns its interface address", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  for (let attempt = 0; attempt < 2; attempt++) {
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), { ticket: "test-ticket", address: "172.25.10.2" });
  }
  assert.equal(readFileSync(join(f.root, "starts"), "utf8"), "--name WSL (Ubuntu)\n");
  const patches = readFileSync(join(f.root, "patches"), "utf8").trim().split("\n");
  assert.equal(patches.length, 2, "only the Host and supervisor need an installed ELF interpreter");
  assert.ok(patches.every(line => !line.endsWith("/patchelf")), "the patcher must never rewrite its own running ELF");
  assert.ok(patches.every(line => line.includes(join(f.env.XDG_CACHE_HOME, "bex", "wsl", "a".repeat(40)))));
});

test("WSL bootstrap hands over a different image and preserves its launch across WSL restarts", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  writeFileSync(join(f.root, "running"), "");
  assert.equal(f.run().status, 0);
  assert.equal(existsSync(join(f.root, "prepared")), true);
  assert.deepEqual(JSON.parse(readFileSync(join(f.root, "resumed-launch"))), {
    executable: "old-host", arguments: ["--name", "Preserved WSL"], working_directory: "/existing/work",
  });
  rmSync(join(f.root, "running"));
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(f.root, "starts"), "utf8"), "resumed\nresumed\n");
});

test("WSL bootstrap rejects a corrupt revision before starting or copying a Host", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  writeFileSync(join(f.source, "revision"), "../../outside");
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Invalid bundled WSL revision/);
  assert.equal(existsSync(join(f.root, "starts")), false);
});

test("an older connected app cannot replace another app's new WSL Host", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  writeFileSync(join(f.root, "running"), "");
  writeFileSync(join(f.root, "image"), "/newer/host-daemon");
  f.env.BEX_WSL_ALLOW_UPGRADE = "0";
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /changed revision/);
  assert.equal(existsSync(join(f.root, "prepared")), false);
  assert.equal(existsSync(join(f.root, "starts")), false);
  assert.equal(existsSync(join(f.root, "running")), true);
});

test("a failed runtime install can be retried without a stale lock or ready marker", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  const patcher = join(f.source, "patchelf");
  const original = readFileSync(patcher);
  writeFileSync(patcher, "#!/bin/sh\nexit 1\n");
  assert.notEqual(f.run().status, 0);
  const ready = join(f.env.XDG_CACHE_HOME, "bex", "wsl", "a".repeat(40), ".ready");
  assert.equal(existsSync(ready), false);
  assert.equal(existsSync(join(f.root, "running")), false);
  writeFileSync(patcher, original);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(existsSync(ready), true);
});

test("an isolated Windows app cannot discover or launch the normal WSL Host", { skip: process.platform !== "linux" }, t => {
  const f = fixture(t);
  f.env.BEX_WSL_SCOPE = "b".repeat(64);
  f.env.XDG_DATA_HOME = join(f.root, "isolated data");
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const launch = readFileSync(join(f.root, "starts"), "utf8");
  assert.ok(launch.startsWith(`--state-dir ${join(f.env.XDG_DATA_HOME, "bex", "wsl-isolated", f.env.BEX_WSL_SCOPE)} --isolated --name WSL (Ubuntu)`));
  f.env.BEX_WSL_SCOPE = "../normal";
  assert.notEqual(f.run().status, 0);
});
