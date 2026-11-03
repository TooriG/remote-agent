#!/bin/sh
# Executed by the selected distro's login shell, with the bundle path as $1.
set -eu
umask 077
source_dir=$(wslpath -u "$1")
revision=$(cat "$source_dir/revision")
case "$revision" in ''|*[!a-f0-9]*) echo 'Invalid bundled WSL revision' >&2; exit 1;; esac
[ "$(uname -m)" = x86_64 ] || { echo 'The bundled WSL Host requires an x86-64 distribution' >&2; exit 1; }
cache_root=${XDG_CACHE_HOME:-$HOME/.cache}/bex/wsl
runtime=$cache_root/$revision
mkdir -p "$cache_root"
exec 9>"$cache_root/install.lock"
flock -w 30 9
if [ ! -f "$runtime/.ready" ]; then
    pending=$(mktemp -d "$cache_root/install.XXXXXX")
    trap 'rm -rf "$pending"' EXIT HUP INT TERM
    cp -R "$source_dir/." "$pending/"
    chmod -R u+rwX "$pending"
    chmod u+x "$pending/host-daemon" "$pending/agent-provider-supervisor" "$pending/patchelf" "$pending/lib/ld-linux-x86-64.so.2"
    # Install at its final path before fixing ELF interpreters. The payload and
    # its libc travel together; a user's distro needs neither Nix nor patchelf.
    mkdir -p "$runtime"
    cp -R "$pending/." "$runtime/"
    # Always invoke patchelf through the bundled loader. Patching its own
    # running ELF would invalidate its memory mappings and can SIGBUS.
    for executable in host-daemon agent-provider-supervisor; do
        "$runtime/lib/ld-linux-x86-64.so.2" --library-path "$runtime/lib" "$runtime/patchelf" \
            --set-interpreter "$runtime/lib/ld-linux-x86-64.so.2" "$runtime/$executable"
    done
    : > "$runtime/.ready"
    rm -rf "$pending"
    trap - EXIT HUP INT TERM
fi
# UDP loopback forwarding is not available in WSL2 NAT mode. The Host listens on
# wildcard sockets, so give Windows the distro's interface address and same port.
address=$(hostname -I | cut -d' ' -f1)
[ -n "$address" ] || { echo 'Cannot determine the WSL address' >&2; exit 1; }
scope=${BEX_WSL_SCOPE:-}
case "$scope" in *[!a-f0-9]*) echo 'Invalid WSL isolation scope' >&2; exit 1;; esac
log=$cache_root/host.log
launch=$cache_root/host-launch.json
if [ -n "$scope" ]; then
    state=${XDG_DATA_HOME:-$HOME/.local/share}/bex/wsl-isolated/$scope
    mkdir -p "$state"
    log=$state/host.log
    launch=$state/host-launch.json
    run_host() { "$runtime/host-daemon" --state-dir "$state" --isolated "$@"; }
else
    run_host() { "$runtime/host-daemon" "$@"; }
fi
access() { run_host local-client --address "$address" --require-current-binary; }
upgrading=0
start() {
    if [ -f "$launch" ]; then
        nohup "$runtime/host-daemon" resume-host --launch-file "$launch" 9>&- </dev/null >>"$log" 2>&1 &
    elif [ -n "$scope" ]; then
        (cd "$HOME"; exec nohup "$runtime/host-daemon" --state-dir "$state" --isolated --name "WSL (${WSL_DISTRO_NAME:-Linux})") 9>&- </dev/null >>"$log" 2>&1 &
    else
        (cd "$HOME"; exec nohup "$runtime/host-daemon" --name "WSL (${WSL_DISTRO_NAME:-Linux})") 9>&- </dev/null >>"$log" 2>&1 &
    fi
}
upgrade() {
    if [ "${BEX_WSL_ALLOW_UPGRADE:-1}" != 1 ]; then
        echo 'The WSL Host changed revision; restart BEX to use the updated Host' >&2
        exit 1
    fi
    # The old Host journals each active conversation/settings and sends one
    # continuation in that same native session when the replacement starts.
    run_host prepare-update >"$launch.pending"
    mv "$launch.pending" "$launch"
    # The detached restarter waits for the old lease. It must survive a closed
    # Windows client after preparation has interrupted its WSL conversations.
    start
    upgrading=1
}
if access 2>/dev/null; then exit 0; else status=$?; fi
if [ "$status" = 42 ]; then upgrade; else start; fi
attempt=0
while [ "$attempt" -lt 100 ]; do
    if access 2>/dev/null; then exit 0; else status=$?; fi
    # Another desktop can have been starting the previous image when this
    # bootstrap began. Upgrade it as soon as it becomes ready, under our lock.
    if [ "$status" = 42 ] && [ "$upgrading" = 0 ]; then upgrade; fi
    attempt=$((attempt + 1))
    sleep 0.2
done
echo "WSL Host did not start; see $log" >&2
exit 1
