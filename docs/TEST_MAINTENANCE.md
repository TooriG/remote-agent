# Test audits

Routine verification policy is in [AGENTS.md](../AGENTS.md); commands are defined
in [justfile](../justfile), and CI coverage in
[native.yml](../.github/workflows/native.yml). The audits below are explicit,
focused checks, separate from ordinary unit tests.

## Mutation testing

Run `scripts/dev-env.sh just mutants-diff` to audit the production diff parser in
an isolated copy. Extra arguments go to cargo-mutants, for example `--list` or
`--re quoted_path`. To audit another component, restrict mutations to its
production code and select the tests that exercise it.

Read `target/mutation-diff/mutants.out/missed.txt` and the corresponding diffs
and logs. Distinguish equivalent mutations from missing assertions; strengthen
assertions that protect observable behavior. Report timeouts and unbuildable
mutants separately from assertion failures. Do not accept changed expectations
to make a mutant pass.

Ordinary proptest runs retain random input generation; the audit fixes its seed
for reproducibility. Commit regression seeds only for failures of unmutated
production code, never for deliberately broken mutants.

## Bounded proofs

Run `nix develop .#kani --command just kani-diff`. The dedicated shell pins Kani,
CBMC and the matching Rust toolchain without changing the global toolchain.

The harnesses in
[proofs.rs](../crates/agent-core/src/presentation/diff/proofs.rs) call the
production decoder and state their input and unwind bounds beside the assertions.
Keep unwinding assertions enabled. Each harness has a 120-second timeout;
timeouts are failures, not proofs. Results go to `target/kani-diff/results.json`.
These finite domains do not prove arbitrary-length decoding, the whole diff
parser or asynchronous application behavior. Retain example, real Git and
property tests for those boundaries.

## Real-time QUIC soak

Dispatch Native clients manually, or explicitly run:

```sh
scripts/dev-env.sh cargo test --locked -p host-fixture --test iroh_host \
  passive_client_can_approve_after_five_minutes_without_reconnecting -- --ignored
```

A virtual clock does not establish that a real connection survives inactivity.
Native acceptance results must include every requested test with no failures or
skips; build success alone does not verify interactions.
