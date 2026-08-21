#!/bin/bash
# c7i.4xlarge — the differential suite, recorded on Linux.
#
# docs/testing.md keeps collecting and testing apart: `cargo test` replays
# checked-in snapshots and spawns nothing, and `jay-corpus record` is the one
# thing that runs a reference interpreter — by hand, on the owner's machine.
# This is a second such machine, and it answers what a Mac cannot: do
# jconsole and GNU APL, on a different OS and libc, answer what the
# snapshots say they answered?
#
# `record --check` re-measures and fails on drift while writing nothing, so
# the run is a gate rather than an edit. A full `record` follows, and the
# diff of the snapshots is uploaded for the owner to read: nothing is
# committed by a machine that is about to delete itself.
#
# Clean-room, not relaxed by any of this: both interpreters are black-box
# subprocesses. Nothing reads their source, nothing links them, neither
# binary enters the repository, and the jconsole tarball is fetched from
# Jsoftware each run rather than mirrored into the owner's bucket. GNU APL is
# GPL source built here; only the BUILD PRODUCT is cached, so the second run
# of this profile skips five minutes of compiling.

set -uo pipefail
#@@CONFIG@@
#@@COMMON@@

J_VERSION=j9.6
J_TARBALL="https://www.jsoftware.com/download/${J_VERSION}/install/${J_VERSION}_linux64.tar.gz"
APL_VERSION=1.9
APL_TARBALL="https://ftp.gnu.org/gnu/apl/apl-${APL_VERSION}.tar.gz"
ORACLES=/opt/oracles
ARCH="$(uname -m)"

install_jconsole() {
	mkdir -p "$ORACLES"
	curl -sSL "$J_TARBALL" -o /tmp/j.tar.gz || return 1
	tar xzf /tmp/j.tar.gz -C "$ORACLES"
	LIBJAY_ORACLE_J="$(find "$ORACLES" -type f -name jconsole | head -1)"
	[ -n "$LIBJAY_ORACLE_J" ] || return 1
	chmod +x "$LIBJAY_ORACLE_J"
	export LIBJAY_ORACLE_J
	printf 'jconsole at %s\n' "$LIBJAY_ORACLE_J"
	printf '2+2\nexit 0\n' | "$LIBJAY_ORACLE_J" -jprofile /dev/null
}

# Cached build product first, source build second, and the product is put
# back so the next run of this profile costs five minutes less.
install_gnu_apl() {
	local cached="s3://$BUCKET/prebuilt/gnu-apl-${APL_VERSION}-${ARCH}.tar.gz"
	mkdir -p "$ORACLES"
	if aws s3 cp "$cached" /tmp/apl.tar.gz --quiet 2>/dev/null; then
		printf 'using the cached GNU APL build at %s\n' "$cached"
		tar xzf /tmp/apl.tar.gz -C "$ORACLES"
	else
		printf 'no cached build; compiling GNU APL %s from the FSF tarball\n' "$APL_VERSION"
		pkg gcc gcc-c++ make ncurses-devel 2>/dev/null ||
			pkg build-essential libncurses-dev || return 1
		curl -sSL "$APL_TARBALL" -o /tmp/apl-src.tar.gz || return 1
		mkdir -p /tmp/aplsrc && tar xzf /tmp/apl-src.tar.gz -C /tmp/aplsrc --strip-components=1
		(cd /tmp/aplsrc && ./configure --prefix="$ORACLES/gnu-apl" >/dev/null &&
			make -j"$(nproc)" >/dev/null && make install >/dev/null) || return 1
		tar czf /tmp/apl.tar.gz -C "$ORACLES" gnu-apl
		aws s3 cp /tmp/apl.tar.gz "$cached" --quiet ||
			printf 'note: could not cache the build; the next run rebuilds\n'
	fi
	LIBJAY_ORACLE_APL="$ORACLES/gnu-apl/bin/apl"
	[ -x "$LIBJAY_ORACLE_APL" ] || return 1
	export LIBJAY_ORACLE_APL
	printf 'GNU APL at %s\n' "$LIBJAY_ORACLE_APL"
	"$LIBJAY_ORACLE_APL" --version | head -2
}

phase inputs fetch_inputs
phase python install_python
phase libjay install_libjay
phase rust-toolchain install_rust
phase jconsole install_jconsole
phase gnu-apl install_gnu_apl

export LIBJAY_ORACLE_J="${LIBJAY_ORACLE_J:-$(find "$ORACLES" -type f -name jconsole | head -1)}"
export LIBJAY_ORACLE_APL="${LIBJAY_ORACLE_APL:-$ORACLES/gnu-apl/bin/apl}"

# The replay first: it needs no interpreter and proves the tree is sound
# before either oracle is asked anything.
phase replay crun test -p libjay --release --test oracle
phase replay-apl crun test -p libjay --release --test oracle_apl

# The gate. Re-measure every recorded expression against the interpreters as
# this Linux box runs them, and fail on any drift, writing nothing.
phase record-check-j crun run -p libjay-devtools --release -- record j --check
phase record-check-apl crun run -p libjay-devtools --release -- record apl --check

# And what a Linux recording WOULD write, as a diff for the owner to read.
# Nothing is committed from here: the machine that produced it is about to
# delete itself.
record_diff() {
	cp -a "$WORK/crates/libjay/tests/snapshots" /tmp/snapshots-before || return 1
	crun run -p libjay-devtools --release -- record j || return 1
	crun run -p libjay-devtools --release -- record apl || return 1
	diff -ru /tmp/snapshots-before "$WORK/crates/libjay/tests/snapshots" \
		>"$RESULTS/snapshot-drift.diff" 2>&1
	local n
	n="$(wc -l <"$RESULTS/snapshot-drift.diff" | tr -d ' ')"
	printf 'a Linux recording differs from the checked-in one by %s diff lines\n' "$n"
	head -200 "$RESULTS/snapshot-drift.diff"
}
phase record-diff record_diff
phase corpus-stats crun run -p libjay-devtools --release -- stats

step "all phases attempted; finish() uploads and terminates"
exit 0
