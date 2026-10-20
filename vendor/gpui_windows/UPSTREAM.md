Vendored `gpui-pre-windows` 0.3.4 from crates.io (SHA-256
`45918b16e62e89ef4952a6e140297f716934bf6f9b21206ad6ce54faa7316005`),
the gpui-pre snapshot of Zed commit `69164008341295ad481bb11c0334a712ca8c23e3`.

The renderer is unchanged. Its build script uses the repository's pinned FXC
package instead of discovering an installed SDK or accepting a PATH/environment
override. It tracks the pin, preparation script and all HLSL inputs so Cargo
recompiles shaders when any of those inputs change. The unused build-time
registry dependency is removed.
