#!/usr/bin/env bash
# End-to-end check that sandboxes never write into the shared image cache.
#
# Drives the production path: an isolated engine, `iii-worker sandbox-daemon`
# (handle_create + IiiWorkerLauncher) and the `iii-worker sandbox` CLI, on a
# real image pulled from its registry. The default image is Red Hat UBI 9:
# /usr/bin, /usr/lib and /root ship without owner write bits and
# /etc/shadow and /etc/gshadow with mode 0000, so the unprivileged daemon has
# to extract into directories it may not write by mode and build the
# read-only erofs lower from files it may not read by mode.
#
# Scenario: sandbox A overwrites an image binary, plants files, edits
# /etc/passwd, renames, deletes and chmods image files, uploads a file through
# sandbox::fs::write and tries to reach the host through the virtio-fs root
# share and the erofs block device. Sandbox B (booted while A is alive) and
# sandbox C (booted after A and B stopped) must see the pristine image, and
# the cache directory and its erofs image must be byte-for-byte unchanged.
#
# Usage:
#   III_BIN=<iii engine> WORKER_BIN=<iii-worker built with embed-init> \
#   [IMAGE_REF=<oci ref>] [WORK=<scratch dir>] [PORT=<ws port>] \
#   scripts/sandbox-isolation-e2e.sh
# Requires Linux with /dev/kvm usable by the current (unprivileged) user.
# If III_LIBKRUNFW_PATH is set it is forwarded; otherwise the worker must
# embed libkrunfw (--features embed-libkrunfw).
set -uo pipefail

: "${III_BIN:?set III_BIN to the iii engine binary}"
: "${WORKER_BIN:?set WORKER_BIN to an iii-worker built with --features embed-init}"
IMAGE_REF=${IMAGE_REF:-registry.access.redhat.com/ubi9/ubi:latest}
PORT=${PORT:-49771}
WORK=${WORK:-$(mktemp -d)}
# The daemon spawns `__vm-boot` from its canonical executable path and derives
# every cache path from HOME; compare against the same absolute spellings.
III_BIN=$(realpath "$III_BIN") WORKER_BIN=$(realpath "$WORKER_BIN")
mkdir -p "$WORK" && WORK=$(cd "$WORK" && pwd)
LOGS=$WORK/logs
SBX_HOME=$WORK/home
mkdir -p "$LOGS" "$SBX_HOME" "$WORK/tmp"
cd "$WORK" || exit 2

FAILURES=0
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*"; FAILURES=$((FAILURES + 1)); }
check() { local desc=$1; shift; if "$@"; then pass "$desc"; else fail "$desc"; fi; }

# Nothing leaks in from the caller's environment (engine URLs, compose vars).
RUN_ENV=(env -i PATH=/usr/local/bin:/usr/bin:/bin HOME="$SBX_HOME" TMPDIR="$WORK/tmp"
  III_TELEMETRY_ENABLED=false DO_NOT_TRACK=1 RUST_LOG=info NO_COLOR=1)
[ -n "${III_LIBKRUNFW_PATH:-}" ] && RUN_ENV+=("III_LIBKRUNFW_PATH=$III_LIBKRUNFW_PATH")
run() { "${RUN_ENV[@]}" "$@"; }

# Root-level read of the cache for the manifests only (0000 files); the
# daemon itself always runs as the current user.
SUDO=()
if [ "$(id -u)" -ne 0 ] && sudo -n true 2> /dev/null; then SUDO=(sudo -n); fi
# Without sudo, files the user cannot read (mode 0000) are compared by
# type/mode/owner/size/mtime only.
manifest() {
  (cd "$1" && "${SUDO[@]}" find . -printf '%p %y %m %u %g %s %T@ %l\n' | LC_ALL=C sort &&
    "${SUDO[@]}" find . -type f -exec sha256sum {} + 2> /dev/null | LC_ALL=C sort)
}

PIDS=()
CREATED=()
cleanup() {
  for id in "${CREATED[@]}"; do
    run timeout 30 "$WORKER_BIN" sandbox stop --port "$PORT" "$id" > /dev/null 2>&1
  done
  local p i
  for p in "${PIDS[@]}"; do kill -TERM "$p" 2> /dev/null; done
  for p in "${PIDS[@]}"; do
    for i in $(seq 1 50); do kill -0 "$p" 2> /dev/null || break; sleep 0.2; done
    kill -KILL "$p" 2> /dev/null
  done
  # Only this run's VMs: each one boots with `--rootfs-lower $WORK/home/...`.
  ps -ww -eo pid=,args= | grep -F -- "$WORKER_BIN __vm-boot" | grep -F -- "--rootfs-lower $WORK/" |
    grep -v -F 'grep -F' | awk '{print $1}' | xargs -r kill -KILL 2> /dev/null
}
trap cleanup EXIT

wait_until() {  # wait_until <seconds> <cmd...>
  local deadline=$((SECONDS + $1)); shift
  until "$@" > /dev/null 2>&1; do
    [ "$SECONDS" -ge "$deadline" ] && return 1
    sleep 0.5
  done
}
port_open() { (exec 3<> "/dev/tcp/127.0.0.1/$PORT") 2> /dev/null; }
daemon_ready() { run timeout 10 "$WORKER_BIN" sandbox list --port "$PORT"; }

create() {  # create <var>: boot a sandbox on the e2e image, store its id in <var>
  local t0=$SECONDS id rc attempt
  # The first create pulls the image; the CLI gives up after the engine's
  # invocation timeout while the daemon keeps pulling, so retry.
  for attempt in 1 2 3; do
    id=$(run timeout 900 "$WORKER_BIN" sandbox create --port "$PORT" e2e-image 2> "$LOGS/create-$1.stderr")
    rc=$?
    echo "create $1 (attempt $attempt): rc=$rc id=$id after $((SECONDS - t0))s"
    { [ "$rc" -eq 0 ] && [ -n "$id" ]; } && break
    grep -q 'invocation timed out' "$LOGS/create-$1.stderr" || break
  done
  if [ "$rc" -ne 0 ] || [ -z "$id" ]; then
    cat "$LOGS/create-$1.stderr"; tail -40 "$LOGS/daemon.log"
    fail "sandbox::create $1"; exit 1
  fi
  CREATED+=("$id")
  printf -v "$1" '%s' "$id"
}
gx() {  # gx <id> <label> <script> [KEY=VALUE...]: sandbox::exec, returns the guest exit code
  local id=$1 label=$2 script=$3 rc; shift 3
  local envs=(); for kv in "$@"; do envs+=(-e "$kv"); done
  echo "----- [guest ${id:0:8}] $label"
  run timeout 300 "$WORKER_BIN" sandbox exec --port "$PORT" "${envs[@]}" "$id" -- /bin/sh -c "$script" 2>&1
  rc=$?
  echo "----- [guest ${id:0:8}] exit=$rc"
  return "$rc"
}
stop_sb() {
  run timeout 60 "$WORKER_BIN" sandbox stop --port "$PORT" "$1" > /dev/null 2>&1
  local left=() id; for id in "${CREATED[@]}"; do [ "$id" = "$1" ] || left+=("$id"); done
  CREATED=("${left[@]}")
}
no_files_in_trampoline() {  # only the staging dirs may exist in a sandbox's trampoline
  [ -d "/tmp/iii-sandbox/$1/root" ] && [ -z "$(find "/tmp/iii-sandbox/$1/root" -mindepth 1 ! -type d -print -quit)" ]
}
host_checks() {  # host_checks <label>
  manifest "$CACHE" > "$LOGS/cache-manifest-$1.txt"
  if cmp -s "$LOGS/cache-manifest-before.txt" "$LOGS/cache-manifest-$1.txt"; then
    pass "[$1] image cache unchanged ($(wc -l < "$LOGS/cache-manifest-$1.txt") manifest lines)"
  else
    fail "[$1] image cache changed"
    diff "$LOGS/cache-manifest-before.txt" "$LOGS/cache-manifest-$1.txt" | head -40
  fi
  check "[$1] erofs lower unchanged" sha256sum -c --quiet "$LOGS/erofs.sha256"
}

echo "===== $(date -Is) image=$IMAGE_REF uid=$(id -u) kernel=$(uname -r)"
[ "${#SUDO[@]}" -gt 0 ] || echo "note: no passwordless sudo; unreadable cache files are not content-hashed"
echo "engine:  $III_BIN ($(sha256sum "$III_BIN" | cut -c1-16))"
echo "worker:  $WORKER_BIN ($(sha256sum "$WORKER_BIN" | cut -c1-16))"
[ "$(id -u)" -ne 0 ] || echo "WARNING: running as root; the unprivileged erofs build path is not exercised"
if [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then fail "/dev/kvm is not usable by $(id -un)"; exit 1; fi

cat > "$WORK/config.yaml" << EOF
workers:
  - name: iii-worker-manager
    config:
      port: $PORT
      host: 127.0.0.1
EOF
cat > "$WORK/sandbox.yaml" << EOF
auto_install: true
image_allowlist:
  - e2e-image
custom_images:
  e2e-image: $IMAGE_REF
default_idle_timeout_secs: 1800
max_concurrent_sandboxes: 4
# vCPUs and memory come from the CLI defaults (1 vCPU, 512 MiB).
EOF

# Started directly (not through a shell function) so $! is the process itself.
"${RUN_ENV[@]}" "$III_BIN" --config "$WORK/config.yaml" --no-update-check > "$LOGS/engine.log" 2>&1 &
PIDS+=($!)
if ! wait_until 120 port_open; then tail -40 "$LOGS/engine.log"; fail "engine did not listen on $PORT"; exit 1; fi
"${RUN_ENV[@]}" "$WORKER_BIN" sandbox-daemon --config "$WORK/sandbox.yaml" \
  --engine "ws://127.0.0.1:$PORT" > "$LOGS/daemon.log" 2>&1 &
PIDS+=($!)
if ! wait_until 120 daemon_ready; then tail -40 "$LOGS/daemon.log"; fail "sandbox daemon did not register"; exit 1; fi
echo "engine on 127.0.0.1:$PORT, sandbox daemon registered"

echo "===== sandbox A: first create (pull + extract + erofs + boot)"
create A
mapfile -t EROFS_FILES < <(find "$SBX_HOME/.iii" -name '*.erofs' -type f)
if [ "${#EROFS_FILES[@]}" -ne 1 ]; then fail "expected one erofs image, found: ${EROFS_FILES[*]}"; exit 1; fi
EROFS=${EROFS_FILES[0]}
CACHE=${EROFS%.erofs}
echo "cache: $CACHE ($("${SUDO[@]}" du -sh "$CACHE" | cut -f1)), erofs: $EROFS ($(du -sh "$EROFS" | cut -f1))"
manifest "$CACHE" > "$LOGS/cache-manifest-before.txt"
sha256sum "$EROFS" > "$LOGS/erofs.sha256"
for f in etc/shadow etc/gshadow usr/bin; do
  [ -e "$CACHE/$f" ] && echo "image mode: /$f $(stat -c '%a %U' "$CACHE/$f")"
done
check "init.krun is not copied into the image cache" test ! -e "$CACHE/init.krun"
VMARGS=$(ps -ww -eo args= | grep -F -- '__vm-boot' | grep -F -- "/tmp/iii-sandbox/$A/root" | grep -v -F 'grep -F' | head -1)
echo "A boot args: $VMARGS"
check "A boots on its own trampoline, served read-only" grep -qF -- '--rootfs-readonly' <<< "$VMARGS"
check "A boots with the erofs image as its overlay lower" grep -qF -- "--rootfs-mode overlay --rootfs-lower $EROFS" <<< "$VMARGS"
check "the cache directory is never handed to the VM" bash -c '! grep -qF -- "--rootfs $1 " <<< "$2 "' _ "$CACHE" "$VMARGS"

read -r -d '' A_IDENTITY << 'EOF'
grep '^PRETTY_NAME=' /etc/os-release
echo "root mounts: $(awk '$2 == "/" {printf "%s ", $3}' /proc/mounts)"
[ "$(awk '$2 == "/" {t = $3} END {print t}' /proc/mounts)" = overlay ] || { echo "guest / is not an overlay"; exit 1; }
[ "$(id -u)" = 0 ] || { echo "not root in the guest"; exit 1; }
ls -l /etc/shadow
grep -q '^root:' /etc/shadow || { echo "guest root cannot read /etc/shadow"; exit 1; }
EOF
check "A: guest root is an overlay and root reads /etc/shadow" gx "$A" "identity" "$A_IDENTITY"

read -r -d '' A_WRITES << 'EOF'
set -e
for c in python3 curl gpg dnf apk apt-get; do p=$(command -v "$c") && break; done
T=$(readlink -f "$p")
echo "TARGET=$T"
printf 'POISON\n' > "$T"
mkdir -p /etc/profile.d
echo 'echo pwned' > /etc/profile.d/zz-sbx-e2e.sh
echo 'evil:x:0:0::/root:/bin/sh' >> /etc/passwd
echo planted > /root/sbx-e2e-planted
chmod 0666 /etc/group
mv /etc/os-release /etc/os-release.moved
rm /etc/shadow
sync
[ "$(cat "$T")" = POISON ]
[ -e /etc/profile.d/zz-sbx-e2e.sh ] && grep -q '^evil:' /etc/passwd && [ -e /root/sbx-e2e-planted ]
[ "$(stat -c %a /etc/group)" = 666 ] && [ -e /etc/os-release.moved ] && [ ! -e /etc/shadow ]
echo "all writes landed inside A"
EOF
A_OUT=$(gx "$A" "attacker writes (must succeed inside A)" "$A_WRITES")
A_RC=$?
echo "$A_OUT"
check "A: the sandbox root stays writable for the guest" test "$A_RC" -eq 0
TARGET=$(sed -n 's/^TARGET=//p' <<< "$A_OUT" | head -1)
# Pristine values come from the manifest taken before A's writes, so a write
# that leaked into the cache cannot make B and C match the poisoned state.
TARGET_SHA=$(awk -v p=".$TARGET" '$2 == p {print $1}' "$LOGS/cache-manifest-before.txt")
GROUP_MODE=$(awk '$1 == "./etc/group" && $2 == "f" {print $3}' "$LOGS/cache-manifest-before.txt")
check "pristine hash and mode were recorded before A's writes" test -n "$TARGET_SHA" -a -n "$GROUP_MODE"
echo "target binary: $TARGET (pristine sha256 ${TARGET_SHA:0:16}), /etc/group mode $GROUP_MODE"

echo "uploaded through sandbox::fs::write" > "$WORK/upload.txt"
check "A: sandbox::fs::write into an image directory works" \
  run timeout 60 "$WORKER_BIN" sandbox upload --port "$PORT" "$A" "$WORK/upload.txt" /usr/bin/sbx-e2e-uploaded
check "A: the uploaded file is visible inside A" gx "$A" "read upload" 'grep -q sandbox::fs::write /usr/bin/sbx-e2e-uploaded'

read -r -d '' A_ESCAPE << 'EOF'
f=0
if command -v mount > /dev/null; then
  mkdir -p /mnt/vfs
  if mount -t virtiofs /dev/root /mnt/vfs 2> /dev/null; then
    if touch /mnt/vfs/sbx-e2e-escape 2> /dev/null; then echo "ESCAPE: created a file on the virtio-fs root share"; f=1; fi
    if mkdir /mnt/vfs/sbx-e2e-escape-dir 2> /dev/null; then echo "ESCAPE: created a dir on the virtio-fs root share"; f=1; fi
    if { echo x > /mnt/vfs/dev/sbx-e2e-escape; } 2> /dev/null; then echo "ESCAPE: wrote under the virtio-fs root share"; f=1; fi
    umount /mnt/vfs
    echo "virtio-fs root share remounted: writes refused"
  else
    echo "virtio-fs root share remount refused"
  fi
else
  echo "SKIP: no mount binary in this image"
fi
for d in /dev/vda /dev/vdb; do
  [ -b "$d" ] || continue
  if dd if=/dev/zero of="$d" bs=512 count=1 conv=notrunc 2> /dev/null; then echo "ESCAPE: wrote to $d"; f=1; else echo "write to $d refused"; fi
done
exit $f
EOF
check "A: no write reaches the host through the root share or the lower device" gx "$A" "escape attempts" "$A_ESCAPE"

read -r -d '' CLEAN << 'EOF'
f=0
leak() { echo "LEAK: $*"; f=1; }
[ "$(sha256sum "$TARGET" | cut -d' ' -f1)" = "$TARGET_SHA" ] || leak "$TARGET was modified"
[ ! -e /etc/profile.d/zz-sbx-e2e.sh ] || leak "/etc/profile.d/zz-sbx-e2e.sh exists"
! grep -q '^evil:' /etc/passwd || leak "/etc/passwd has the planted entry"
[ ! -e /root/sbx-e2e-planted ] || leak "/root/sbx-e2e-planted exists"
[ "$(stat -c %a /etc/group)" = "$GROUP_MODE" ] || leak "/etc/group mode changed"
{ [ -e /etc/os-release ] && [ ! -e /etc/os-release.moved ]; } || leak "/etc/os-release was renamed"
grep -q '^root:' /etc/shadow || leak "/etc/shadow was deleted"
[ ! -e /usr/bin/sbx-e2e-uploaded ] || leak "/usr/bin/sbx-e2e-uploaded exists"
[ "$f" = 0 ] && echo "pristine image"
exit $f
EOF
CLEAN_ENV=("TARGET=$TARGET" "TARGET_SHA=$TARGET_SHA" "GROUP_MODE=$GROUP_MODE")

echo "===== sandbox B: created while A is alive"
create B
check "B sees none of A's writes" gx "$B" "pristine image" "$CLEAN" "${CLEAN_ENV[@]}"
host_checks "A and B alive"
check "A's trampoline holds no files" no_files_in_trampoline "$A"
check "B's trampoline holds no files" no_files_in_trampoline "$B"
stop_sb "$A"
stop_sb "$B"

echo "===== sandbox C: created after A and B stopped"
create C
check "C inherits none of A's writes" gx "$C" "pristine image" "$CLEAN" "${CLEAN_ENV[@]}"
stop_sb "$C"
host_checks "after all sandboxes"

echo "===== daemon timings"
grep -E 'create_phase|boot_phase' "$LOGS/daemon.log" | sed -E 's/\x1b\[[0-9;]*m//g' | head -30

echo "===== result: $FAILURES failure(s)"
[ "$FAILURES" -eq 0 ]
