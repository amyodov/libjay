# The owner's account facts, and the caps every script in this directory
# obeys. Sourced by launch.sh and fetch-results.sh; never executed on its
# own, never uploaded anywhere, and it holds NO credentials — the keys live
# in ~/.aws on the owner's Mac and nowhere else.
#
# Every value spelled OWNER-… is the owner's to fill in once. While any of
# them remains, `assert_configured` below refuses to let a script run.
#
# shellcheck shell=bash

# ---------------------------------------------------------------- identity

# The named profile in ~/.aws/credentials holding the dedicated IAM user's
# keys. Nothing here ever reads a key; the AWS CLI does, from this profile.
: "${LIBJAY_AWS_PROFILE:=libjay-bench}"

# The account those keys belong to. Checked against sts:GetCallerIdentity
# before anything is launched: a profile pointing somewhere else is a stop.
: "${LIBJAY_ACCOUNT_ID:=OWNER-ACCOUNT-ID}"

# The IAM user name the keys must belong to. Refusing to run as anyone else
# (root, an admin, a different user) is what keeps the blast radius the one
# this design reasons about.
: "${LIBJAY_IAM_USER:=libjay-bench-launcher}"

# The one region. The IAM policy pins it too; this is the copy the scripts
# read. us-east-2 is the suggestion: cheap, deep spot capacity for both c7i
# and g5, and every AMI used here is published there.
: "${LIBJAY_REGION:=OWNER-REGION}"

# ------------------------------------------------------------------ places

# The one bucket. Block Public Access on, versioning off, one lifecycle rule
# aborting incomplete multipart uploads. Nothing else in the account writes
# to it and the IAM policies reach no other bucket.
: "${LIBJAY_BUCKET:=OWNER-BUCKET-NAME}"

# The CloudWatch log group, created once by the owner (the instance role may
# write streams into it but may not create it).
: "${LIBJAY_LOG_GROUP:=/libjay/bench}"

# The VPC whose subnets the instance lands in — the account's default VPC is
# fine. The launcher picks the subnet in the cheapest availability zone.
: "${LIBJAY_VPC_ID:=OWNER-VPC-ID}"

# The one security group the IAM policy allows: no ingress at all, egress
# only to the ports a bootstrap needs. Created once by the owner.
: "${LIBJAY_SECURITY_GROUP_ID:=OWNER-SECURITY-GROUP-ID}"

# The instance profile carrying the instance role. Separate identity from
# the launching user: it can write one S3 prefix and one log group, and has
# no EC2 permission of any kind.
: "${LIBJAY_INSTANCE_PROFILE:=libjay-bench-instance}"

# The monthly AWS Budget the spend guard reads before launching.
: "${LIBJAY_BUDGET_NAME:=libjay-monthly}"

# ------------------------------------------------------------------- caps

# The tag every launched instance carries. Both the IAM policy's terminate
# permission and the concurrency check key off it.
: "${LIBJAY_PROJECT_TAG:=libjay-bench}"

# Hard ceiling on a run's life, in minutes. The instance shuts itself down
# at this mark by three independent timers, and the shutdown terminates it.
# The launcher clamps any --max-minutes to this; raising it here is the only
# way to raise it, and 240 is the number the cost bound in README.md uses.
: "${LIBJAY_MAX_MINUTES_CEILING:=240}"

# Month-to-date spend, in whole dollars, above which the launcher refuses.
# Read two ways — the Budget's reported actual, and the sum of this
# directory's own ledger for the month — and the larger wins.
: "${LIBJAY_SPEND_GUARD_USD:=15}"

# Root volume. gp3, deleted on termination, and the IAM policy caps the size.
: "${LIBJAY_VOLUME_GB:=60}"
: "${LIBJAY_VOLUME_TYPE:=gp3}"

# ---------------------------------------------------------------- profiles
#
# One line per profile: instance type, spot price cap in $/hour, default
# runtime in minutes, and the SSM public parameter naming the AMI.
#
# The price cap is what goes into the spot request, so it is also the first
# half of the max-spend arithmetic printed before every launch. AWS never
# charges above the on-demand rate for a spot instance, so the cap is an
# upper bound whatever the market does.

libjay_profile_type() {
	case "$1" in
	avx512) echo "c7i.4xlarge" ;;
	graviton) echo "c7g.4xlarge" ;;
	gpu) echo "g5.xlarge" ;;
	oracle) echo "c7i.4xlarge" ;;
	*) return 1 ;;
	esac
}

libjay_profile_max_price() {
	case "$1" in
	avx512 | oracle) echo "0.36" ;;
	graviton) echo "0.30" ;;
	gpu) echo "0.55" ;;
	*) return 1 ;;
	esac
}

libjay_profile_minutes() {
	case "$1" in
	avx512) echo "150" ;;
	graviton) echo "120" ;;
	gpu) echo "120" ;;
	oracle) echo "60" ;;
	*) return 1 ;;
	esac
}

libjay_profile_ami_parameter() {
	case "$1" in
	avx512 | oracle) echo "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-6.1-x86_64" ;;
	graviton) echo "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-6.1-arm64" ;;
	# Ubuntu 22.04 with the NVIDIA driver and CUDA already installed, so no
	# kernel module is built on the instance and the GPU profile's bootstrap
	# is apt plus a Vulkan loader rather than a driver install.
	gpu) echo "/aws/service/deeplearning/ami/x86_64/base-oss-nvidia-driver-gpu-ubuntu-22.04/latest/ami-id" ;;
	*) return 1 ;;
	esac
}

# shellcheck disable=SC2034  # read by launch.sh, which sources this file
LIBJAY_PROFILES="avx512 graviton gpu oracle"

# ------------------------------------------------------------------ guards

# Refuse while any OWNER-… placeholder is still in place. Called by every
# script before it does anything at all.
assert_configured() {
	local unset_names=() name value
	for name in LIBJAY_ACCOUNT_ID LIBJAY_REGION LIBJAY_BUCKET LIBJAY_VPC_ID \
		LIBJAY_SECURITY_GROUP_ID; do
		value="${!name}"
		case "$value" in
		OWNER-*) unset_names+=("$name") ;;
		esac
	done
	if [ ${#unset_names[@]} -ne 0 ]; then
		printf 'bench/cloud is not configured yet.\n\n' >&2
		printf 'Still holding an OWNER placeholder:\n' >&2
		printf '  %s\n' "${unset_names[@]}" >&2
		printf '\nFill them in bench/cloud/config.sh (or export them), after\n' >&2
		printf 'running the one-time setup in bench/cloud/README.md.\n' >&2
		return 1
	fi
}
