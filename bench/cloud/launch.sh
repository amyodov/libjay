#!/usr/bin/env bash
#
# One spot instance, one run, then it deletes itself.
#
#   bench/cloud/launch.sh avx512
#   bench/cloud/launch.sh gpu --wheel pypi:0.2.0
#   bench/cloud/launch.sh graviton --dry-run
#
# Nothing here needs a human at the keyboard, so everything a confirmation
# prompt would have caught is a check instead: see "Preflight" below and the
# guardrail table in README.md. The script reads credentials only through the
# named AWS profile in ~/.aws, and refuses to run as any identity but the
# dedicated one.
#
# What it never does: launch on demand, launch a second instance while one is
# running, launch without a self-destruct timer, or launch without first
# writing what the run may cost into the ledger.

set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd -- "$HERE/../.." && pwd)"
# shellcheck source=bench/cloud/config.sh
. "$HERE/config.sh"

die() {
	printf 'launch: %s\n' "$*" >&2
	exit 1
}
say() { printf '%s\n' "$*"; }
rule() { printf -- '----------------------------------------------------------------\n'; }

# ------------------------------------------------------------------ arguments

PROFILE=""
MAX_MINUTES=""
MAX_PRICE=""
WHEEL="gh-latest"
ROWS=""
QUICK=0
DRY_RUN=0
WATCH=1

usage() {
	cat <<'EOF'
usage: launch.sh <profile> [options]

profiles
  avx512     c7i.4xlarge   AVX-512: the v4 clones run for the first time
  graviton   c7g.4xlarge   Graviton3: the first ARM numbers
  gpu        g5.xlarge     NVIDIA A10G: the first execution of the f64 shaders
  oracle     c7i.4xlarge   jconsole + GNU APL, differential recording

options
  --max-minutes N     life of the instance; clamped to the ceiling in config.sh
  --max-price P       spot cap in $/hour; the profile's default is the bound
                      README.md quotes
  --wheel SPEC        gh-latest (default) | gh-run:<id> | pypi:<version>
                      | /path/to/*.whl | none  (build on the instance)
  --rows N            row count handed to the bench scripts
  --quick             the short form of every script; a smoke, not a result
  --dry-run           run every check and the EC2 dry-run, launch nothing
  --no-watch          launch and return; fetch-results.sh collects later
EOF
}

[ $# -gt 0 ] || {
	usage
	exit 2
}
PROFILE="$1"
shift
case "$PROFILE" in
-h | --help)
	usage
	exit 0
	;;
esac
while [ $# -gt 0 ]; do
	case "$1" in
	--max-minutes)
		MAX_MINUTES="${2:-}"
		shift 2
		;;
	--max-price)
		MAX_PRICE="${2:-}"
		shift 2
		;;
	--wheel)
		WHEEL="${2:-}"
		shift 2
		;;
	--rows)
		ROWS="${2:-}"
		shift 2
		;;
	--quick)
		QUICK=1
		shift
		;;
	--dry-run)
		DRY_RUN=1
		shift
		;;
	--no-watch)
		WATCH=0
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option $1" ;;
	esac
done

case " $LIBJAY_PROFILES " in
*" $PROFILE "*) ;;
*) die "unknown profile '$PROFILE'; one of: $LIBJAY_PROFILES" ;;
esac

TYPE="$(libjay_profile_type "$PROFILE")"
: "${MAX_PRICE:=$(libjay_profile_max_price "$PROFILE")}"
: "${MAX_MINUTES:=$(libjay_profile_minutes "$PROFILE")}"
AMI_PARAM="$(libjay_profile_ami_parameter "$PROFILE")"
TEMPLATE="$HERE/userdata-$PROFILE.sh"
[ -f "$TEMPLATE" ] || die "no user-data template at $TEMPLATE"

# The ceiling is not an argument. Anything larger is clamped, loudly.
if [ "$MAX_MINUTES" -gt "$LIBJAY_MAX_MINUTES_CEILING" ]; then
	say "note: --max-minutes $MAX_MINUTES clamped to the ceiling ${LIBJAY_MAX_MINUTES_CEILING}"
	MAX_MINUTES="$LIBJAY_MAX_MINUTES_CEILING"
fi
[ "$MAX_MINUTES" -ge 10 ] || die "--max-minutes must be at least 10"

# ------------------------------------------------------------------ preflight

assert_configured || exit 1

for tool in aws jq git; do
	command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done

ec2() { aws --profile "$LIBJAY_AWS_PROFILE" --region "$LIBJAY_REGION" ec2 "$@"; }
s3() { aws --profile "$LIBJAY_AWS_PROFILE" --region "$LIBJAY_REGION" s3 "$@"; }
ssmp() { aws --profile "$LIBJAY_AWS_PROFILE" --region "$LIBJAY_REGION" ssm "$@"; }

say "preflight"

# 1. The identity. A key for anyone but the dedicated user is a stop: every
#    bound this design claims rests on that user's policy and no other.
caller="$(aws --profile "$LIBJAY_AWS_PROFILE" sts get-caller-identity --output json)" ||
	die "sts:GetCallerIdentity failed for profile '$LIBJAY_AWS_PROFILE'"
account="$(printf '%s' "$caller" | jq -r .Account)"
arn="$(printf '%s' "$caller" | jq -r .Arn)"
[ "$account" = "$LIBJAY_ACCOUNT_ID" ] ||
	die "profile '$LIBJAY_AWS_PROFILE' is account $account, expected $LIBJAY_ACCOUNT_ID"
case "$arn" in
*":user/$LIBJAY_IAM_USER") ;;
*) die "profile '$LIBJAY_AWS_PROFILE' is $arn, expected the user $LIBJAY_IAM_USER — refusing to launch under a wider identity" ;;
esac
say "  identity      $arn"

# 2. Concurrency one. A second live instance is either a run in progress or a
#    run whose orchestrator died; either way this script does not add to it.
# shellcheck disable=SC2016  # the backticks are JMESPath, not a subshell
live="$(ec2 describe-instances \
	--filters "Name=tag:Project,Values=$LIBJAY_PROJECT_TAG" \
	"Name=instance-state-name,Values=pending,running,stopping,stopped" \
	--query 'Reservations[].Instances[].[InstanceId,InstanceType,Tags[?Key==`RunId`]|[0].Value,LaunchTime]' \
	--output text)"
if [ -n "$live" ]; then
	printf 'launch: an instance tagged %s is already alive:\n' "$LIBJAY_PROJECT_TAG" >&2
	printf '%s\n' "$live" >&2
	printf 'launch: concurrency is one. Wait for it, or terminate it:\n' >&2
	printf '  aws --profile %s --region %s ec2 terminate-instances --instance-ids <id>\n' \
		"$LIBJAY_AWS_PROFILE" "$LIBJAY_REGION" >&2
	exit 1
fi
say "  concurrency   nothing else tagged $LIBJAY_PROJECT_TAG is alive"

# 3. Spend. Two readings, and the larger one decides.
#
#    The Budget's actual covers everything the account spends, including a
#    launch this directory knows nothing about — but AWS refreshes it a few
#    times a day, so it is a slow leak detector, not a brake.
#
#    The ledger is every launch this script has made this month, at the bound
#    it printed, and it is exact the instant it is written. Between them they
#    cover the fast case and the wide case; neither covers both.
month="$(date -u +%Y-%m)"
budget_actual=""
if budget_json="$(aws --profile "$LIBJAY_AWS_PROFILE" --region us-east-1 budgets describe-budget \
	--account-id "$LIBJAY_ACCOUNT_ID" --budget-name "$LIBJAY_BUDGET_NAME" --output json 2>/dev/null)"; then
	budget_actual="$(printf '%s' "$budget_json" |
		jq -r '.Budget.CalculatedSpend.ActualSpend.Amount // empty')"
fi
ledger_total=0
if ledger_keys="$(s3 ls "s3://$LIBJAY_BUCKET/ledger/$month/" 2>/dev/null | awk '{print $4}')"; then
	for key in $ledger_keys; do
		[ -n "$key" ] || continue
		amount="$(s3 cp "s3://$LIBJAY_BUCKET/ledger/$month/$key" - 2>/dev/null |
			jq -r '.max_spend_usd // 0')" || amount=0
		ledger_total="$(awk -v a="$ledger_total" -v b="$amount" 'BEGIN{printf "%.2f", a+b}')"
	done
fi
say "  spend         budget actual ${budget_actual:-unavailable}, ledger bound $ledger_total (month $month)"
guard_hit=0
if [ -n "$budget_actual" ] &&
	awk -v a="$budget_actual" -v g="$LIBJAY_SPEND_GUARD_USD" 'BEGIN{exit !(a>g)}'; then
	guard_hit=1
	say "  spend         BUDGET ACTUAL $budget_actual is over the guard $LIBJAY_SPEND_GUARD_USD"
fi
if awk -v a="$ledger_total" -v g="$LIBJAY_SPEND_GUARD_USD" 'BEGIN{exit !(a>g)}'; then
	guard_hit=1
	say "  spend         LEDGER BOUND $ledger_total is over the guard $LIBJAY_SPEND_GUARD_USD"
fi
if [ "$guard_hit" = 1 ]; then
	die "month-to-date spend is over the guard; raise LIBJAY_SPEND_GUARD_USD deliberately or wait for the month to turn"
fi
if [ -z "$budget_actual" ]; then
	say "  spend         note: the budget could not be read; only the ledger is guarding this launch"
fi

# 4. The market. Cheapest availability zone that both offers the type and has
#    a subnet, and a refusal if the going rate is already above the cap — a
#    request above the market never fills and a request below it never runs.
offered="$(ec2 describe-instance-type-offerings --location-type availability-zone \
	--filters "Name=instance-type,Values=$TYPE" \
	--query 'InstanceTypeOfferings[].Location' --output text)"
[ -n "$offered" ] || die "$TYPE is offered in no availability zone of $LIBJAY_REGION"
subnet_rows="$(ec2 describe-subnets --filters "Name=vpc-id,Values=$LIBJAY_VPC_ID" \
	--query 'Subnets[].[AvailabilityZone,SubnetId]' --output text)"
[ -n "$subnet_rows" ] || die "no subnets in $LIBJAY_VPC_ID"
price_rows="$(ec2 describe-spot-price-history --instance-types "$TYPE" \
	--product-descriptions "Linux/UNIX" --start-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
	--query 'SpotPriceHistory[].[AvailabilityZone,SpotPrice]' --output text)"
[ -n "$price_rows" ] || die "no spot price history for $TYPE in $LIBJAY_REGION"

AZ=""
SUBNET=""
SPOT_NOW=""
while read -r az price; do
	[ -n "$az" ] || continue
	case " $offered " in *" $az "*) ;; *) continue ;; esac
	subnet="$(printf '%s\n' "$subnet_rows" | awk -v a="$az" '$1==a {print $2; exit}')"
	[ -n "$subnet" ] || continue
	if [ -z "$SPOT_NOW" ] || awk -v p="$price" -v b="$SPOT_NOW" 'BEGIN{exit !(p<b)}'; then
		AZ="$az"
		SUBNET="$subnet"
		SPOT_NOW="$price"
	fi
done <<<"$price_rows"
[ -n "$AZ" ] || die "no availability zone offers $TYPE and has a subnet in $LIBJAY_VPC_ID"
awk -v p="$SPOT_NOW" -v c="$MAX_PRICE" 'BEGIN{exit !(p<=c)}' ||
	die "spot is \$$SPOT_NOW/h in $AZ, above the cap \$$MAX_PRICE/h — raise --max-price deliberately or come back later"
say "  market        $TYPE in $AZ at \$$SPOT_NOW/h, cap \$$MAX_PRICE/h, subnet $SUBNET"

# 5. The image, and its root device name, which differs between the Amazon
#    Linux profiles and the GPU profile's Ubuntu.
AMI="$(ssmp get-parameter --name "$AMI_PARAM" --query 'Parameter.Value' --output text)"
[ -n "$AMI" ] || die "could not resolve $AMI_PARAM"
ROOT_DEVICE="$(ec2 describe-images --image-ids "$AMI" \
	--query 'Images[0].RootDeviceName' --output text)"
[ -n "$ROOT_DEVICE" ] && [ "$ROOT_DEVICE" != "None" ] || die "no root device on $AMI"
say "  image         $AMI ($AMI_PARAM), root $ROOT_DEVICE"

# ------------------------------------------------------------------ the plan

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$PROFILE"
S3_RUN="s3://$LIBJAY_BUCKET/runs/$RUN_ID"
COMMIT="$(git -C "$REPO" rev-parse HEAD)"
DIRTY=""
git -C "$REPO" diff --quiet HEAD -- 2>/dev/null || DIRTY=" (working tree dirty)"
EXPIRES="$(date -u -v "+${MAX_MINUTES}M" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null ||
	date -u -d "+${MAX_MINUTES} minutes" +%Y-%m-%dT%H:%M:%SZ)"

# The bound this run may cost, and the arithmetic behind it, printed before
# anything is launched and written to the ledger whether or not it is.
HOURS="$(awk -v m="$MAX_MINUTES" 'BEGIN{printf "%.4f", m/60}')"
MAX_SPEND="$(awk -v p="$MAX_PRICE" -v h="$HOURS" -v g="$LIBJAY_VOLUME_GB" \
	'BEGIN{printf "%.2f", p*h + g*0.08/730*h + 0.10}')"

rule
say "run           $RUN_ID"
say "profile       $PROFILE on $TYPE, $AZ"
say "commit        $COMMIT$DIRTY"
say "wheel         $WHEEL"
say "life          $MAX_MINUTES minutes, self-destruct at $EXPIRES"
say "spot cap      \$$MAX_PRICE/h (market \$$SPOT_NOW/h; AWS never bills spot above on demand)"
say "MAX SPEND     \$$MAX_SPEND = \$$MAX_PRICE/h x ${HOURS}h + ${LIBJAY_VOLUME_GB}GB gp3 + \$0.10 transfer"
say "results       $S3_RUN/"
say "log group     $LIBJAY_LOG_GROUP, stream $RUN_ID"
rule

# ------------------------------------------------ the request, and a rehearsal

STAGE="$(mktemp -d)"
cleanup_stage() { rm -rf "$STAGE"; }
trap cleanup_stage EXIT

BDM="$(jq -nc --arg dev "$ROOT_DEVICE" --argjson gb "$LIBJAY_VOLUME_GB" \
	--arg vt "$LIBJAY_VOLUME_TYPE" \
	'[{DeviceName:$dev,Ebs:{VolumeSize:$gb,VolumeType:$vt,DeleteOnTermination:true,Encrypted:true}}]')"
TAGS="ResourceType=instance,Tags=[{Key=Project,Value=$LIBJAY_PROJECT_TAG},{Key=RunId,Value=$RUN_ID},{Key=Profile,Value=$PROFILE},{Key=Expires,Value=$EXPIRES},{Key=MaxMinutes,Value=$MAX_MINUTES}]"
VTAGS="ResourceType=volume,Tags=[{Key=Project,Value=$LIBJAY_PROJECT_TAG},{Key=RunId,Value=$RUN_ID}]"
MARKET="MarketType=spot,SpotOptions={MaxPrice=$MAX_PRICE,SpotInstanceType=one-time,InstanceInterruptionBehavior=terminate}"

run_instances() {
	ec2 run-instances "$@" \
		--image-id "$AMI" \
		--instance-type "$TYPE" \
		--count 1 \
		--subnet-id "$SUBNET" \
		--security-group-ids "$LIBJAY_SECURITY_GROUP_ID" \
		--iam-instance-profile "Name=$LIBJAY_INSTANCE_PROFILE" \
		--instance-initiated-shutdown-behavior terminate \
		--instance-market-options "$MARKET" \
		--block-device-mappings "$BDM" \
		--metadata-options "HttpEndpoint=enabled,HttpTokens=required,HttpPutResponseHopLimit=1" \
		--tag-specifications "$TAGS" "$VTAGS" \
		--user-data "file://$USERDATA"
}

# The permission rehearsal, before anything is uploaded and before the ledger
# is written: EC2 evaluates the whole request against the policy and creates
# nothing. The user-data is a stub here — its content plays no part in the
# policy decision, and the real one is rendered below and checked separately.
printf '#!/bin/bash\nexit 0\n' >"$STAGE/stub-userdata.sh"
USERDATA="$STAGE/stub-userdata.sh"
if run_instances --dry-run >/dev/null 2>"$STAGE/dryrun.err"; then
	die "run-instances --dry-run returned success, which it never should"
fi
if ! grep -q "DryRunOperation" "$STAGE/dryrun.err"; then
	printf 'launch: the EC2 dry run was refused, so the real launch would be too:\n' >&2
	cat "$STAGE/dryrun.err" >&2
	exit 1
fi
say "  permission    the EC2 dry run says this exact request is allowed"

# ------------------------------------------------------------------ staging
#
# The instance has no GitHub credential and no git. Everything it needs is put
# in its own S3 prefix from here, under the owner's own `gh` login, and the
# instance reads that prefix and nothing else.
#
# Every input is pinned by digest in the user-data, which nothing but this
# process can write. The launching user can write the run prefix, so a stolen
# key could in principle replace a staged wheel between the upload and the
# boot; a digest the instance checks makes that swap useless.
sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

WHEEL_SPEC="$WHEEL"
WHEEL_SHA=""
case "$WHEEL" in
none | pypi:*) ;;
gh-latest | gh-run:*)
	command -v gh >/dev/null 2>&1 || die "--wheel $WHEEL needs the gh CLI"
	case "$TYPE" in
	c7g.*) artifact="wheel-ubuntu-latest-aarch64" ;;
	*) artifact="wheel-ubuntu-latest-x86_64" ;;
	esac
	if [ "$WHEEL" = "gh-latest" ]; then
		run_json="$(gh run list --repo amyodov/libjay --workflow publish.yml \
			--status success --limit 1 --json databaseId,headSha,createdAt)"
		run_id_gh="$(printf '%s' "$run_json" | jq -r '.[0].databaseId // empty')"
		head_sha="$(printf '%s' "$run_json" | jq -r '.[0].headSha // empty')"
		[ -n "$run_id_gh" ] || die "no successful publish.yml run to take a wheel from"
		if [ "$head_sha" != "$COMMIT" ]; then
			say "note: the wheel is from $head_sha, this tree is $COMMIT — the numbers"
			say "      measure the wheel's commit. Run publish.yml on this commit first"
			say "      (gh workflow run publish.yml --ref \$(git rev-parse --abbrev-ref HEAD))"
			say "      if the difference matters."
		fi
	else
		run_id_gh="${WHEEL#gh-run:}"
		head_sha="$(gh run view --repo amyodov/libjay "$run_id_gh" --json headSha -q .headSha)"
	fi
	say "staging       wheel from publish.yml run $run_id_gh ($artifact, $head_sha)"
	gh run download --repo amyodov/libjay "$run_id_gh" -n "$artifact" -D "$STAGE/wheel" ||
		die "gh run download failed; the artifact expires after 90 days"
	whl="$(find "$STAGE/wheel" -name '*.whl' | head -1)"
	[ -n "$whl" ] || die "no .whl in artifact $artifact"
	s3 cp "$whl" "$S3_RUN/input/$(basename "$whl")" >/dev/null
	WHEEL_SPEC="s3:input/$(basename "$whl")"
	WHEEL_SHA="$(sha256 "$whl")"
	;;
*.whl)
	[ -f "$WHEEL" ] || die "no such wheel: $WHEEL"
	s3 cp "$WHEEL" "$S3_RUN/input/$(basename "$WHEEL")" >/dev/null
	WHEEL_SPEC="s3:input/$(basename "$WHEEL")"
	WHEEL_SHA="$(sha256 "$WHEEL")"
	;;
*) die "unrecognised --wheel spec: $WHEEL" ;;
esac

# The source tree, for the bench scripts, the corpora and — on the profiles
# whose gate is a Rust test — cargo itself. One tarball, from git, so what
# runs is exactly what the commit says and nothing from the working tree.
git -C "$REPO" archive --format=tar.gz --prefix=libjay/ -o "$STAGE/source.tar.gz" HEAD
s3 cp "$STAGE/source.tar.gz" "$S3_RUN/input/source.tar.gz" >/dev/null
SOURCE_SHA="$(sha256 "$STAGE/source.tar.gz")"
say "staging       source.tar.gz ($(wc -c <"$STAGE/source.tar.gz" | tr -d ' ') bytes) at $COMMIT"

# ------------------------------------------------------------------ user-data

COMMON="$HERE/userdata-common.sh"
[ -f "$COMMON" ] || die "no $COMMON to splice into the template"
USERDATA="$STAGE/userdata.sh"   # from here on, the real one
{
	while IFS= read -r line; do
		if [ "$line" = "#@@COMMON@@" ]; then
			cat "$COMMON"
		elif [ "$line" = "#@@CONFIG@@" ]; then
			cat <<EOF
RUN_ID='$RUN_ID'
PROFILE='$PROFILE'
BUCKET='$LIBJAY_BUCKET'
REGION='$LIBJAY_REGION'
LOG_GROUP='$LIBJAY_LOG_GROUP'
MAX_MINUTES='$MAX_MINUTES'
WHEEL_SPEC='$WHEEL_SPEC'
COMMIT='$COMMIT'
ROWS='$ROWS'
QUICK='$QUICK'
SOURCE_SHA='$SOURCE_SHA'
WHEEL_SHA='$WHEEL_SHA'
EOF
		else
			printf '%s\n' "$line"
		fi
	done <"$TEMPLATE"
} >"$USERDATA"
grep -q "^RUN_ID=" "$USERDATA" || die "$TEMPLATE has no #@@CONFIG@@ line to fill"
grep -q "^trap finish EXIT" "$USERDATA" || die "$TEMPLATE has no #@@COMMON@@ line to fill"
bash -n "$USERDATA" || die "the rendered user-data is not valid bash"

# EC2 caps user data at 16 KB. A template that outgrows it should stage itself
# in S3 and leave a bootstrap here — but the self-destruct must stay inline,
# ahead of any network call, whatever else moves.
udsize="$(wc -c <"$USERDATA" | tr -d ' ')"
[ "$udsize" -le 16000 ] || die "user-data is $udsize bytes, over EC2's 16 KB limit"
say "staging       user-data $udsize bytes"

# ------------------------------------------------------------------ ledger

ledger="$STAGE/ledger.json"
jq -nc \
	--arg run_id "$RUN_ID" --arg profile "$PROFILE" --arg type "$TYPE" --arg az "$AZ" \
	--arg ami "$AMI" --arg commit "$COMMIT" --arg wheel "$WHEEL_SPEC" \
	--arg spot_now "$SPOT_NOW" --arg max_price "$MAX_PRICE" --arg expires "$EXPIRES" \
	--argjson max_minutes "$MAX_MINUTES" --argjson max_spend "$MAX_SPEND" \
	--arg launched_by "$arn" --arg planned_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
	--argjson dry_run "$DRY_RUN" \
	'{run_id:$run_id,profile:$profile,instance_type:$type,az:$az,ami:$ami,
	  commit:$commit,wheel:$wheel,spot_price_at_launch:$spot_now,max_price:$max_price,
	  max_minutes:$max_minutes,expires:$expires,max_spend_usd:$max_spend,
	  launched_by:$launched_by,planned_at:$planned_at,dry_run:($dry_run==1),
	  instance_id:null}' >"$ledger"
s3 cp "$ledger" "s3://$LIBJAY_BUCKET/ledger/$month/$RUN_ID.json" >/dev/null
say "  ledger        s3://$LIBJAY_BUCKET/ledger/$month/$RUN_ID.json, bound \$$MAX_SPEND"

if [ "$DRY_RUN" = 1 ]; then
	rule
	say "--dry-run: every check passed and nothing was launched."
	say "The staged inputs are at $S3_RUN/input/ and the ledger records a dry run."
	exit 0
fi

# ------------------------------------------------------------------ launch

INSTANCE_ID="$(run_instances --query 'Instances[0].InstanceId' --output text)"
[ -n "$INSTANCE_ID" ] || die "run-instances returned no instance id"
say "launched      $INSTANCE_ID"

# The whole cost story rests on one instance attribute. Read it back, and if
# EC2 did not take it, terminate now rather than trust a timer.
behaviour="$(ec2 describe-instance-attribute --instance-id "$INSTANCE_ID" \
	--attribute instanceInitiatedShutdownBehavior \
	--query 'InstanceInitiatedShutdownBehavior.Value' --output text)"
if [ "$behaviour" != "terminate" ]; then
	ec2 terminate-instances --instance-ids "$INSTANCE_ID" >/dev/null || true
	die "shutdown behaviour is '$behaviour', not 'terminate' — instance terminated"
fi
say "verified      instance-initiated shutdown terminates"

jq --arg id "$INSTANCE_ID" '.instance_id=$id' "$ledger" >"$ledger.2" && mv "$ledger.2" "$ledger"
s3 cp "$ledger" "s3://$LIBJAY_BUCKET/ledger/$month/$RUN_ID.json" >/dev/null

if [ "$WATCH" = 0 ]; then
	rule
	say "Not watching. Collect with:"
	say "  bench/cloud/fetch-results.sh $RUN_ID"
	exit 0
fi

# ------------------------------------------------------------------ watching
#
# Read-only from here on: the status object and the rolling log the instance
# syncs to S3. The instance does not need this process to be alive, and this
# process cannot make the instance live longer.

DEADLINE=$(($(date +%s) + MAX_MINUTES * 60 + 600))
seen=0
status="(none yet)"
partial="$STAGE/console.log"

interrupt() {
	rule
	say "Detached. $INSTANCE_ID keeps running and destroys itself by $EXPIRES."
	say "  bench/cloud/fetch-results.sh $RUN_ID"
	say "  aws --profile $LIBJAY_AWS_PROFILE --region $LIBJAY_REGION ec2 terminate-instances --instance-ids $INSTANCE_ID"
	exit 0
}
trap interrupt INT

rule
say "watching $RUN_ID; Ctrl-C detaches without stopping the run"
while :; do
	sleep 15
	if s3 cp "$S3_RUN/log/console.log" "$partial" --quiet >/dev/null 2>&1; then
		total="$(wc -l <"$partial" | tr -d ' ')"
		if [ "$total" -gt "$seen" ]; then
			sed -n "$((seen + 1)),\$p" "$partial"
			seen="$total"
		fi
	fi
	new_status="$(s3 cp "$S3_RUN/STATUS" - 2>/dev/null || true)"
	[ -n "$new_status" ] && status="$new_status"
	case "$status" in
	done | failed*)
		rule
		say "status: $status"
		break
		;;
	esac
	state="$(ec2 describe-instances --instance-ids "$INSTANCE_ID" \
		--query 'Reservations[0].Instances[0].State.Name' --output text 2>/dev/null || echo unknown)"
	case "$state" in
	terminated | shutting-down)
		rule
		say "instance $state; last status was '$status'"
		break
		;;
	esac
	if [ "$(date +%s)" -gt "$DEADLINE" ]; then
		say "past the deadline by ten minutes with the instance still $state — terminating"
		ec2 terminate-instances --instance-ids "$INSTANCE_ID" >/dev/null || true
		break
	fi
done

rule
say "collect:  bench/cloud/fetch-results.sh $RUN_ID"
