// Exercise the distributed ELF payload, not the builder's Nix store binaries.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, readlinkSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const source = resolve(process.argv[2]);
const scope = mkdtempSync(join(tmpdir(), "bex-wsl-runtime-"));
const tools = join(scope, "tools");
mkdirSync(tools);
// Linux CI has no wsl.exe filesystem bridge. All other bootstrap steps run as shipped.
writeFileSync(join(tools, "wslpath"), '#!/bin/sh\nprintf "%s\\n" "$2"\n', { mode: 0o700 });
const env = { ...process.env, PATH: `${tools}:${process.env.PATH}`,
  XDG_DATA_HOME: join(scope, "data"), XDG_CACHE_HOME: join(scope, "cache"),
  XDG_CONFIG_HOME: join(scope, "config"), CODEX_HOME: join(scope, "codex"),
  CLAUDE_CONFIG_DIR: join(scope, "claude"), WSL_DISTRO_NAME: "Ubuntu" };
const revision = readFileSync(join(source, "revision"), "utf8").trim();
const runtime = join(env.XDG_CACHE_HOME, "bex", "wsl", revision);
const loader = join(runtime, "lib", "ld-linux-x86-64.so.2");
const host = join(runtime, "host-daemon");
const run = (file, args, options = {}) => execFileSync(file, args, { env, timeout: 60000, ...options });
try {
  const reply = run("sh", ["crates/agent-local-host/src/wsl-bootstrap.sh", source]);
  const access = JSON.parse(reply);
  assert.equal(access.secret.length, 32);
  assert.ok(access.ticket);
  reply.fill(0);
  const status = JSON.parse(run(host, ["status"]));
  assert.equal(status.name, "WSL (Ubuntu)");
  const dependencies = run(loader, ["--list", host], { encoding: "utf8", env: { ...env, LD_LIBRARY_PATH: "" } });
  assert.ok(!dependencies.includes("/nix/store"), dependencies);
  const interpreter = run(loader, ["--library-path", join(runtime, "lib"), join(runtime, "patchelf"), "--print-interpreter", host], { encoding: "utf8" }).trim();
  assert.equal(interpreter, loader);
  console.log("The installed WSL Host authenticates; its interpreter and libraries use only the bundled runtime.");
} finally {
  const processes = readdirSync("/proc").filter(name => /^\d+$/.test(name)).filter(pid => {
    try { return readlinkSync(`/proc/${pid}/exe`) === host; } catch { return false; }
  });
  for (const pid of processes) {
    try { process.kill(Number(pid), "SIGINT"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  }
  for (let attempt = 0; attempt < 100; attempt++) {
    const alive = processes.some(pid => { try { process.kill(Number(pid), 0); return true; } catch { return false; } });
    if (!alive) break;
    if (attempt === 99) throw new Error("The isolated WSL Host did not stop");
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  rmSync(scope, { recursive: true, force: true });
}
