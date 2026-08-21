#!/usr/bin/env bash
#
# Bring a run's output down from S3 and say what arrived.
#
#   bench/cloud/fetch-results.sh                       list the recent runs
#   bench/cloud/fetch-results.sh 20260822T101500Z-gpu  fetch that one
#   bench/cloud/fetch-results.sh latest --tail         fetch the newest, tail it
#
# Read-only against AWS: it downloads objects and lists prefixes, and touches
# no instance. Results land under bench/cloud/results/<run-id>/, which is
# git-ignored — a number enters the repository only when the owner puts it
# into bench/README.md by hand, with the provenance block those tables carry.

set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bench/cloud/config.sh
. "$HERE/config.sh"

die() {
	printf 'fetch-results: %s\n' "$*" >&2
	exit 1
}
assert_configured || exit 1
command -v aws >/dev/null 2>&1 || die "aws is not on PATH"

s3() { aws --profile "$LIBJAY_AWS_PROFILE" --region "$LIBJAY_REGION" s3 "$@"; }

RUN="${1:-}"
TAIL=0
[ "${2:-}" = "--tail" ] && TAIL=1

list_runs() {
	s3 ls "s3://$LIBJAY_BUCKET/runs/" | awk '{print $2}' | tr -d '/'
}

if [ -z "$RUN" ]; then
	printf 'runs in s3://%s/runs/\n\n' "$LIBJAY_BUCKET"
	for r in $(list_runs); do
		status="$(s3 cp "s3://$LIBJAY_BUCKET/runs/$r/STATUS" - 2>/dev/null | tr -d '\n')"
		printf '  %-32s %s\n' "$r" "${status:-(no status)}"
	done
	printf '\nfetch one:  %s <run-id>\n' "$0"
	exit 0
fi

if [ "$RUN" = latest ]; then
	RUN="$(list_runs | sort | tail -1)"
	[ -n "$RUN" ] || die "no runs in s3://$LIBJAY_BUCKET/runs/"
fi

DEST="$HERE/results/$RUN"
mkdir -p "$DEST"
s3 sync "s3://$LIBJAY_BUCKET/runs/$RUN/" "$DEST/" --exclude 'input/*' ||
	die "nothing at s3://$LIBJAY_BUCKET/runs/$RUN/"

status="$(cat "$DEST/STATUS" 2>/dev/null | tr -d '\n' || true)"
printf '\nrun     %s\nstatus  %s\nlocal   %s\n\n' "$RUN" "${status:-unknown}" "$DEST"

[ -f "$DEST/results/machine.txt" ] && cat "$DEST/results/machine.txt"

if [ -d "$DEST/results" ]; then
	printf '\nphases\n'
	for f in "$DEST"/results/*.txt; do
		[ -f "$f" ] || continue
		name="$(basename "$f" .txt)"
		case "$name" in machine | MANIFEST) continue ;; esac
		verdict="$(grep -c 'FAILED' "$f" 2>/dev/null || true)"
		printf '  %-26s %6s lines  %s\n' "$name" \
			"$(wc -l <"$f" | tr -d ' ')" \
			"$([ "${verdict:-0}" = 0 ] && echo ok || echo 'has FAILED lines')"
	done
fi

# The ledger says what the run was allowed to cost. It is not a bill — the
# bill is in Cost Explorer, a day later — but it is the number this process
# committed to before launching, and the two should never be far apart.
month="${RUN%%T*}"
month="${month:0:4}-${month:4:2}"
if s3 cp "s3://$LIBJAY_BUCKET/ledger/$month/$RUN.json" "$DEST/ledger.json" --quiet 2>/dev/null; then
	printf '\nledger  bound $%s for up to %s minutes at $%s/h\n' \
		"$(jq -r .max_spend_usd "$DEST/ledger.json")" \
		"$(jq -r .max_minutes "$DEST/ledger.json")" \
		"$(jq -r .max_price "$DEST/ledger.json")"
fi

if [ "$TAIL" = 1 ] && [ -f "$DEST/log/console.log" ]; then
	printf '\n--- console.log, last 80 lines ---\n'
	tail -80 "$DEST/log/console.log"
fi

printf '\nfull log: %s/log/console.log\n' "$DEST"
