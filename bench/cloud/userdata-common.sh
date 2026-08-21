# The part every profile shares; launch.sh splices it in at #@@COMMON@@.
# The order is the safety argument: the three shutdown timers are armed
# before the first network call, so an instance that hangs, loses its
# orchestrator or fails every later line still terminates.
#
# shellcheck shell=bash
# shellcheck disable=SC2329  # the helpers below are invoked through `phase`

# --- 1. self-destruct, before anything else ------------------------------
#
# `shutdown -h` here means terminate: the launcher set
# instance-initiated-shutdown-behavior. Three independent timers, because
# each covers the other's failure — a killed shell, a confused systemd.
shutdown -h "+${MAX_MINUTES}" </dev/null >/dev/null 2>&1 || true
systemd-run --on-active="$((MAX_MINUTES + 2))m" /sbin/poweroff -f >/dev/null 2>&1 || true
setsid nohup /bin/bash -c "sleep $(((MAX_MINUTES + 4) * 60)); /sbin/poweroff -f" \
	</dev/null >/dev/null 2>&1 &

# --- 2. one log, and the names everything else uses ----------------------
export AWS_DEFAULT_REGION="$REGION" AWS_RETRY_MODE=standard AWS_MAX_ATTEMPTS=6
export DEBIAN_FRONTEND=noninteractive HOME=/root PATH="/root/.local/bin:$PATH"
LOGFILE=/var/log/libjay-run.log
RESULTS=/var/lib/libjay-results
WORK=/opt/libjay
S3_RUN="s3://$BUCKET/runs/$RUN_ID"
FAILED=0
mkdir -p "$RESULTS" "$WORK"
: >"$LOGFILE"
exec > >(tee -a "$LOGFILE") 2>&1

step() { printf '\n=== [%s] %s\n' "$(date -u +%H:%M:%SZ)" "$*"; }
set_status() {
	printf '%s\n' "$1" >/tmp/STATUS
	aws s3 cp /tmp/STATUS "$S3_RUN/STATUS" --quiet >/dev/null 2>&1 || true
}
sync_log() { aws s3 cp "$LOGFILE" "$S3_RUN/log/console.log" --quiet >/dev/null 2>&1 || true; }

IMDS_TOKEN="$(curl -sS -X PUT -m 5 http://169.254.169.254/latest/api/token \
	-H 'X-aws-ec2-metadata-token-ttl-seconds: 21600' 2>/dev/null || true)"
imds() {
	curl -sS -m 5 -H "X-aws-ec2-metadata-token: $IMDS_TOKEN" \
		"http://169.254.169.254/latest/meta-data/$1" 2>/dev/null
}

# --- 3. the two package managers, behind one name ------------------------
OS_ID="$(. /etc/os-release && echo "$ID")"
pkg() {
	case "$OS_ID" in
	amzn | fedora | rhel) dnf -y install "$@" ;;
	ubuntu | debian) apt-get install -y "$@" ;;
	*) return 1 ;;
	esac
}
pkg_refresh() {
	case "$OS_ID" in
	amzn | fedora | rhel) dnf -y makecache ;;
	ubuntu | debian) apt-get update ;;
	*) return 1 ;;
	esac
}

# Observability cannot start before the AWS CLI exists, and the Ubuntu image
# the GPU profile uses has none. This is the one stretch with no channel out.
pkg_refresh >/dev/null 2>&1 || true
pkg tar gzip unzip curl jq python3 >/dev/null 2>&1 || true
if ! command -v aws >/dev/null 2>&1; then
	curl -sSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o /tmp/awscli.zip &&
		unzip -q /tmp/awscli.zip -d /tmp && /tmp/aws/install >/dev/null 2>&1
fi
command -v aws >/dev/null 2>&1 || {
	/sbin/poweroff -f
	exit 1
}

# --- 4. the log, mirrored where the orchestrator can read it -------------
#
# S3 is the primary channel — the watcher and fetch-results.sh read it, and
# it needs nothing the instance does not already have. CloudWatch mirrors it
# for the console; every call to it is best-effort.
aws logs create-log-stream --log-group-name "$LOG_GROUP" \
	--log-stream-name "$RUN_ID" >/dev/null 2>&1 || true
CW_OFFSET=/tmp/cw.offset
cat >/usr/local/bin/libjay-cw-events.py <<'PY'
"""New whole lines of the run log, as a PutLogEvents batch on stdout.

Resumes from the byte offset in the state file, records the new one, and
exits non-zero when there is nothing to send — the common case, not an error.
"""
import json, sys, time

log, off = sys.argv[1], sys.argv[2]
try:
    start = int(open(off).read().strip() or 0)
except Exception:
    start = 0
data = open(log, "rb").read()
end = data.rfind(b"\n") + 1
if end <= start:
    sys.exit(1)
lines = [x for x in data[start:end].decode("utf-8", "replace").split("\n")[:-1] if x.strip()]
open(off, "w").write(str(end))
if not lines:
    sys.exit(1)
t = int(time.time() * 1000)
json.dump([{"timestamp": t, "message": x[:200000]} for x in lines[:1000]], sys.stdout)
PY
cw_ship() {
	python3 /usr/local/bin/libjay-cw-events.py "$LOGFILE" "$CW_OFFSET" \
		>/tmp/cw-events.json 2>/dev/null || return 0
	[ -s /tmp/cw-events.json ] || return 0
	aws logs put-log-events --log-group-name "$LOG_GROUP" --log-stream-name "$RUN_ID" \
		--log-events file:///tmp/cw-events.json >/dev/null 2>&1 || true
}

# The pump: the log out every half minute, and a look at the spot
# interruption notice. Two minutes of warning flushes what is measured.
(
	while :; do
		sleep 30
		sync_log
		cw_ship
		if curl -sf -m 3 -H "X-aws-ec2-metadata-token: $IMDS_TOKEN" \
			http://169.254.169.254/latest/meta-data/spot/instance-action >/dev/null 2>&1; then
			printf '\n!!! spot interruption notice - flushing what we have\n' >>"$LOGFILE"
			set_status interrupted
			aws s3 cp "$RESULTS" "$S3_RUN/results/" --recursive --quiet >/dev/null 2>&1 || true
			sync_log
			cw_ship
		fi
	done
) &
PUMP=$!

# --- 5. what a run leaves behind -----------------------------------------
finish() {
	rc=$?
	kill "$PUMP" >/dev/null 2>&1 || true
	step "finishing (script exit $rc, phases failed: $FAILED)"
	if [ "$rc" = 0 ] && [ "$FAILED" = 0 ]; then
		set_status "done"
	else
		set_status "failed:$rc:$FAILED"
	fi
	ls -l "$RESULTS" >"$RESULTS/MANIFEST.txt" 2>/dev/null || true
	aws s3 cp "$RESULTS" "$S3_RUN/results/" --recursive --quiet >/dev/null 2>&1 || true
	sync_log
	cw_ship
	sleep 2
	/sbin/poweroff -f
}
trap finish EXIT

# A phase's output goes to the console and to a file of its own, the file is
# uploaded the moment it finishes, and a failure does not stop the run: a
# profile that dies in its fourth phase still delivers three.
phase() {
	local name="$1"
	shift
	step "$name"
	set_status "$name"
	local out="$RESULTS/$name.txt" rc=0
	"$@" 2>&1 | tee "$out"
	rc="${PIPESTATUS[0]}"
	if [ "$rc" = 0 ]; then
		printf 'phase %s: ok\n' "$name"
	else
		printf 'phase %s: FAILED with exit %s\n' "$name" "$rc"
		FAILED=1
	fi
	aws s3 cp "$out" "$S3_RUN/results/$name.txt" --quiet >/dev/null 2>&1 || true
	sync_log
	return 0
}

# --- 6. the machine, on the record ---------------------------------------
set_status booting
step "libjay cloud run $RUN_ID, profile $PROFILE, commit $COMMIT"
{
	printf 'run          %s\n' "$RUN_ID"
	printf 'profile      %s\n' "$PROFILE"
	printf 'commit       %s\n' "$COMMIT"
	printf 'wheel        %s\n' "$WHEEL_SPEC"
	printf 'instance     %s %s in %s\n' "$(imds instance-id)" "$(imds instance-type)" \
		"$(imds placement/availability-zone)"
	printf 'ami          %s\n' "$(imds ami-id)"
	printf 'life         %s minutes from boot\n' "$MAX_MINUTES"
	printf 'kernel       %s\n' "$(uname -srm)"
	printf 'os           %s\n' "$(. /etc/os-release && echo "$PRETTY_NAME")"
	printf 'cpus         %s\n' "$(nproc)"
	printf 'memory       %s\n' "$(awk '/MemTotal/{printf "%.1f GiB", $2/1048576}' /proc/meminfo)"
	printf 'cpu flags    %s\n' "$(grep -m1 '^flags\|^Features' /proc/cpuinfo | cut -c1-600)"
} | tee "$RESULTS/machine.txt"

# --- 7. the tree and the wheel, from S3 and from nowhere else ------------
#
# Both are pinned by the digest the launcher computed before uploading. The
# run prefix is writable by the launching user, so an object swapped between
# the upload and this boot would otherwise execute here; a digest the
# launcher put in the user-data, which nothing else can write, closes it.
verify_sha() {
	[ -n "$2" ] || return 0
	local got
	got="$(sha256sum "$1" | awk '{print $1}')"
	[ "$got" = "$2" ] || {
		printf 'CHECKSUM MISMATCH on %s\n  got      %s\n  expected %s\n' "$1" "$got" "$2"
		return 1
	}
	printf 'sha256 ok  %s\n' "$1"
}

fetch_inputs() {
	aws s3 cp "$S3_RUN/input/source.tar.gz" /tmp/source.tar.gz --quiet || return 1
	verify_sha /tmp/source.tar.gz "$SOURCE_SHA" || return 1
	tar xzf /tmp/source.tar.gz -C "$WORK" --strip-components=1
	printf 'source tree at %s, %s files\n' "$WORK" "$(find "$WORK" -type f | wc -l)"
}

# uv brings its own CPython, so neither image's system Python decides the
# version: 3.12, because numba has no wheel above it.
install_python() {
	curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
	uv venv --python 3.12 "$WORK/.venv"
	"$WORK/.venv/bin/python" -V
}

# One of three, and only the third needs a compiler:
#   pypi:X.Y.Z      the released wheel, the artifact a user gets
#   s3:input/*.whl  the wheel publish.yml built for this commit
#   none            build it here (the escape hatch, ~10 minutes)
install_libjay() {
	local spec="$WHEEL_SPEC"
	case "$spec" in
	pypi:*)
		VIRTUAL_ENV="$WORK/.venv" uv pip install "libjay==${spec#pypi:}" || return 1
		;;
	s3:*)
		aws s3 cp "$S3_RUN/${spec#s3:}" /tmp/libjay.whl --quiet || return 1
		verify_sha /tmp/libjay.whl "$WHEEL_SHA" || return 1
		VIRTUAL_ENV="$WORK/.venv" uv pip install /tmp/libjay.whl || return 1
		;;
	none)
		install_rust || return 1
		VIRTUAL_ENV="$WORK/.venv" uv pip install maturin || return 1
		(cd "$WORK" && VIRTUAL_ENV="$WORK/.venv" "$WORK/.venv/bin/maturin" develop --release) || return 1
		;;
	*)
		printf 'unrecognised wheel spec %s\n' "$spec"
		return 1
		;;
	esac
	VIRTUAL_ENV="$WORK/.venv" uv pip install numpy polars 'numba>=0.60' || return 1
	"$WORK/.venv/bin/python" -c 'import jay; assert jay.j("(+/ % #) 1 2 3 4") == 2.5; print("cold import ok")'
}

install_rust() {
	command -v cargo >/dev/null 2>&1 && return 0
	local want
	want="$(sed -n 's/^ *channel *= *"\(.*\)"/\1/p' "$WORK/rust-toolchain.toml" | head -1)"
	curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
		--default-toolchain "${want:-stable}" >/dev/null || return 1
	# shellcheck disable=SC1091
	. "$HOME/.cargo/env"
	rustc -V
}

# The bench scripts import `jay` and each other, so they run from the tree
# with the venv's interpreter and nothing on PYTHONPATH.
py() { (cd "$WORK" && "$WORK/.venv/bin/python" "$@"); }
crun() {
	# shellcheck disable=SC1091
	. "$HOME/.cargo/env" 2>/dev/null || true
	(cd "$WORK" && cargo "$@")
}

# What the launcher passed through, as arrays so an empty value adds no
# argument at all. --quick is workloads.py's alone; --rows most scripts take.
BENCH_ARGS=()
WORKLOAD_ARGS=()
[ -n "$ROWS" ] && BENCH_ARGS+=(--rows "$ROWS")
[ "$QUICK" = 1 ] && WORKLOAD_ARGS+=(--quick)
