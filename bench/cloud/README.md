# Cloud runs

One spot instance, one benchmark or validation run, then the instance
deletes itself. It exists because three things this project has written
down cannot be measured on the machine that wrote them:

- **AVX-512 has never executed.** Every x86-64 artifact carries 83 v4 clones
  and `nm` finds them, but bench/README.md says "built-but-unmeasured" and
  docs/decisions.md says "no machine on hand has AVX-512".
- **The GPU f64 path has never executed anywhere.** naga type-checks the
  generated WGSL under `Capabilities::FLOAT64` in a unit test; the only
  adapter on hand is behind Metal, which has no `double` in shaders at all.
- **There are no ARM numbers, and the linux-aarch64 wheel has never been
  run.** publish.yml cross-compiles it with `smoke: false`, because the
  runner that builds it cannot execute it.

A fourth thing is cheap to add once the machinery exists: recording the
differential corpus against jconsole and GNU APL on Linux, which is a second
opinion on snapshots recorded only on macOS so far.

**Nothing in this directory has been run.** No AWS resource exists, no
credential exists, and every script refuses to start while an `OWNER-`
placeholder remains in `config.sh`. The one-time setup below is the owner's
to perform; after that the orchestrator launches unattended.

```
config.sh              the owner's account facts and the caps; no credentials
launch.sh              preflight, spot request, read-only watch
userdata-common.sh     spliced into every template: timers, log pump, upload
userdata-avx512.sh     c7i.4xlarge — the v4 rung, for real
userdata-graviton.sh   c7g.4xlarge — the first ARM numbers
userdata-gpu.sh        g5.xlarge   — the first f64 shader execution
userdata-oracle.sh     c7i.4xlarge — jconsole + GNU APL on Linux
fetch-results.sh       bring a run down from S3 and say what arrived
policy/                the IAM documents, applied verbatim by the setup below
```

---

## Concern 1: the credentials

### The identity

A dedicated IAM user, `libjay-bench-launcher`, with one inline policy
(`policy/launcher.json`) and no console password. Its access key lives in
`~/.aws/credentials` on the owner's Mac under the profile `libjay-bench`,
and nowhere else: not in the repository, not in GitHub Actions, not in an
environment file, not in any agent's context. `launch.sh` reads it only by
naming the profile to the AWS CLI, and refuses to run if
`sts:GetCallerIdentity` reports any identity but that user — running under
an admin profile by accident would silently void every bound below.

The instance is a **second, separate identity**: the role
`libjay-bench-instance` (`policy/instance-role.json`), which can write one
S3 prefix and one log group and has no EC2 permission at all. The launching
user's only IAM permission is `iam:PassRole` for that one role, to
`ec2.amazonaws.com` and nothing else. Neither identity can reach the other's
powers.

IMDSv2 is required on the instance (`HttpTokens=required`, hop limit 1), so
a server-side request forgery inside anything the run executes cannot read
the role's credentials the easy way.

### What a thief with the keys COULD do

Honestly, and in the order a thief would care:

1. **Burn compute.** They control user-data, so they can run anything on a
   launched instance — mining, scanning, a proxy — over the ports the
   security group allows out (80, 443, 53, 123). The self-destruct in
   `userdata-*.sh` is a cost control for *our* runs; a thief writes their
   own user-data and has no timer. What bounds them instead:

   - **only four instance types**, `c7i.4xlarge`, `c7g.4xlarge`,
     `g5.xlarge`, `g6.xlarge`, and **only in one region**;
   - **spot only** — an explicit `Deny` on `ec2:RunInstances` when
     `ec2:InstanceMarketType` is anything but `spot`, which also fires when
     the key is absent, i.e. on demand. AWS never bills a spot instance
     above the on-demand rate, so the on-demand rate is a hard ceiling;
   - **only Amazon-published AMIs** (`ec2:Owner` = `amazon`), so a
     pre-baked mining image cannot be launched — only cooked up in
     user-data;
   - **only the one security group and the one VPC**, which they cannot
     create, modify or open ingress on;
   - **an account spot vCPU quota** the owner lowers as a setup step: 16
     vCPU for Standard spot, 4 for G spot. That is the only mechanism that
     limits how MANY instances run at once — IAM has no condition key for
     it — and it is what turns "unbounded" into a number.

   With those quotas, the worst simultaneous burn is one 16-vCPU compute
   instance plus one 4-vCPU GPU instance: at the on-demand ceiling, about
   **\$1.72/hour, \$41/day**. The Budget alerts below are what make "for a
   month" implausible; realistic exposure is hours, i.e. tens of dollars.

2. **Read a little.** `ec2:Describe*` is not resource-restrictable, so they
   see the region's instance, subnet, security-group, volume and tag
   inventory — an information leak worth naming, useful for reconnaissance
   against a wider attack. They can also read the three S3 prefixes (bench
   results, a staged source tarball, wheels — all public or about to be),
   the run log group, and the budget's name and current spend.

3. **Write to three S3 prefixes**, `runs/`, `ledger/` and `prebuilt/`. The
   sharpest version of this is swapping a staged wheel between the upload
   and the boot, so that our own instance executes their code. That is
   closed: `launch.sh` computes the sha256 of the wheel and the source
   tarball and puts both digests in the user-data, which only it can write,
   and the instance refuses an input whose digest does not match. Vandalism
   of past results remains possible; deletion does not — neither identity
   has `s3:DeleteObject`.

4. **Terminate our tagged instances.** A denial of service against a
   benchmark run, costing a relaunch.

### What a thief with the keys COULD NOT do

- Create or modify any IAM user, role, policy, access key or login profile;
  assume any role; touch permissions boundaries. Explicitly denied on top of
  not being granted.
- Launch on demand; or launch through Fleet, Spot Fleet, `RequestSpotInstances`,
  scheduled instances, capacity reservations, launch templates, Auto Scaling,
  Batch, ECS, EKS, EMR, SageMaker, Lambda or Lightsail — every one of which
  would sidestep the `RunInstances` conditions. Explicitly denied.
- Launch any other instance type, in any other region, from any non-Amazon
  AMI, into any other security group or VPC, or with a root volume over
  200 GB or not gp3.
- Create a security group, VPC, subnet, NAT gateway, snapshot, image or
  volume; allocate a host; open any ingress; modify a running instance's
  attributes.
- Reach any other S3 bucket, or any other prefix of this one.
- Write to CloudWatch Logs, create a log group, or read any other log group.
- Read Cost Explorer, change the budget, or touch billing, the account or
  the organization.
- Reach any other AWS service at all: nothing else is granted, and IAM
  denies by default.

### What would be tighter, and is not done

The security group's egress is 80/443/53/123 to anywhere, because the
bootstrap needs PyPI, apt, the Jsoftware and GNU tarballs, and the NVIDIA
packages. Mining and proxying both work over 443, so that is the hole the
compute bound above exists to cover. **If every input were mirrored into the
bucket**, egress could be cut to the S3 gateway endpoint's prefix list plus
the CloudWatch endpoint, and a stolen key would launch instances with no
route to the internet — worthless for abuse. That is a real and reachable
end state; it costs an apt/PyPI mirror in S3 and is listed under Open
questions rather than done, because it trades a maintenance burden for a
risk the quotas already bound.

A **permissions boundary** on the user, carrying the same allow set, would
survive someone attaching a managed policy to it by mistake. One line in the
setup; worth adding if the account ever has more than one administrator.

### The honest framing

The keys live on one Mac. The plausible theft path is that Mac being
compromised — at which point the attacker also holds the owner's own
credentials, and these keys are the least of it. The policy is written to be
minimal anyway, because it costs nothing to write it that way and because
the *accident* case (a script with a bug, an agent with a wrong argument) is
far more likely than the theft case and is bounded by exactly the same
conditions.

---

## Concern 2: runaway cost

### The hard bound

A spot request carries a maximum price; AWS never bills a spot instance
above the on-demand rate, so the cap is an upper bound whatever the market
does. Multiplied by a lifetime that three independent timers enforce, that
is an arithmetic bound per run, and `launch.sh` prints it before launching
and writes it to the ledger:

| profile | type | cap $/h | default life | **max spend, default** | **max spend at the 4 h ceiling** |
|---|---|---:|---:|---:|---:|
| avx512 | c7i.4xlarge | 0.36 | 150 min | **$1.02** | **$1.57** |
| graviton | c7g.4xlarge | 0.30 | 120 min | **$0.71** | **$1.33** |
| gpu | g5.xlarge | 0.55 | 120 min | **$1.21** | **$2.33** |
| oracle | c7i.4xlarge | 0.36 | 60 min | **$0.47** | **$1.57** |

The arithmetic is `cap × hours + 60 GB gp3 for those hours + $0.10 for
transfer`. **No single run can cost more than $2.33**, and with concurrency
pinned at one, no two runs overlap. A week of one run per profile is under
$4.

### Self-destruct, belt and braces

The first thing every rendered user-data does, before any network call:

```sh
shutdown -h "+${MAX_MINUTES}"                                  # systemd, scheduled
systemd-run --on-active="$((MAX_MINUTES + 2))m" /sbin/poweroff -f
setsid nohup bash -c "sleep …; /sbin/poweroff -f" &            # no systemd needed
```

and the instance was launched with
`--instance-initiated-shutdown-behavior terminate`, so any of those three
poweroffs is a termination, not a stop. `launch.sh` reads that attribute
back after the launch and terminates the instance immediately if EC2 did not
take it — the entire cost story rests on that one attribute, so it is
verified rather than assumed. The root volume is
`DeleteOnTermination: true`, so no EBS orphan survives the instance.

Termination therefore survives: the orchestrator dying, the network
dropping, a hung benchmark, a wedged shell, a failed bootstrap, and the
instance being unreachable in every way (there is no SSH and no SSM to
reach it with).

There is no TTL sweeper, because a sweeper is infrastructure — a Lambda, a
role, an EventBridge rule — and this design owns none. The `Expires` tag is
written for a human reading the console, not for a robot. The bound above
is what stands in for a sweeper, and it is a stronger statement: not "we
will clean up", but "this cannot cost more than $2.33".

### The guardrails that replace a per-launch confirmation

The owner rejected a prompt: the orchestrator is an agent and runs
unattended. Everything a prompt would have caught is a check instead, and a
failed check is a refusal, not a warning.

| # | guard | where | what it catches |
|---|---|---|---|
| 1 | placeholders still present → refuse | `config.sh` | a half-configured checkout launching anything |
| 2 | caller must be `libjay-bench-launcher` in the expected account | `launch.sh` | launching under an admin profile, where no bound holds |
| 3 | **concurrency one**: refuse if any instance tagged `Project=libjay-bench` is alive | `launch.sh` | a stuck run, a crashed orchestrator, two agents at once |
| 4 | **spend guard**: refuse if the Budget's actual OR the month's ledger sum exceeds `$15` | `launch.sh` | a slow leak, and a fast one this directory caused |
| 5 | lifetime clamped to `LIBJAY_MAX_MINUTES_CEILING` (240) | `launch.sh` | an argument typo turning 2 h into 2 000 |
| 6 | spot only, twice: `--instance-market-options` and an IAM `Deny` | both | an on-demand launch at 3× the price |
| 7 | market probe: read the live spot price, refuse above the cap | `launch.sh` | a request that would never fill, or a market spike |
| 8 | `run-instances --dry-run` before anything is written | `launch.sh` | a policy that would have refused the real call |
| 9 | ledger written to S3 **before** the launch | `launch.sh` | a launch nobody can account for afterwards |
| 10 | shutdown behaviour read back, terminate if not `terminate` | `launch.sh` | the one attribute the cost bound rests on |
| 11 | three shutdown timers, armed first | user-data | every way a run can hang |
| 12 | watcher terminates at deadline + 10 min | `launch.sh` | a timer that somehow did not fire, while the watcher lives |
| 13 | account spot vCPU quotas (16 Standard, 4 G) | owner setup | how many instances can exist at once, by anyone |
| 14 | AWS Budget at $20/month with alerts at 50%/80%/100% | owner setup | everything else |

Two honest notes about #4. The Budget's `CalculatedSpend.ActualSpend`
refreshes a few times a day, so it detects a slow leak and not a fast one.
The ledger is exact the instant it is written but only covers launches this
script made. Together they cover the wide case and the fast case; neither
covers both, and that is why #13 and #14 exist. Cost Explorer
(`ce:GetCostAndUsage`) was considered and rejected: it costs $0.01 per
request, needs a permission of its own, and its data is no fresher than the
Budget's.

---

## Profiles

| profile | instance | vCPU / RAM | purpose | expected wall | spot band (list on demand) |
|---|---|---|---|---|---|
| `avx512` | c7i.4xlarge | 16 / 32 GiB | the x86-64-v4 rung executed and measured; the whole of `bench/` on 16 real cores | 70–110 min | $0.22–0.32 ($0.714) |
| `graviton` | c7g.4xlarge | 16 / 32 GiB | the first ARM numbers; the first execution of the cross-compiled aarch64 wheel | 60–100 min | $0.18–0.28 ($0.578) |
| `gpu` | g5.xlarge | 4 / 16 GiB, A10G | the first f64 shader execution anywhere; the FP64 validation run | 60–100 min | $0.30–0.45 ($1.006) |
| `oracle` | c7i.4xlarge | 16 / 32 GiB | jconsole and GNU APL on Linux; `record --check` as a gate, a full recording as a diff | 30–50 min | $0.22–0.32 ($0.714) |

On-demand figures are list prices to re-check — the market moves and
`launch.sh` reads it live anyway, refusing above the cap.

**`avx512`.** Two halves that want different artifacts. The numbers come
from the wheel publish.yml built, because that is the artifact a user gets:
`bench/simd.py --levels baseline,v2,v3,v4` puts the v4 clones through the
same table bench/README.md already carries for v2 and v3. The correctness
comes from `tests/simd.rs`, which holds every level to bit-identical
elementwise results and 1e-12 on reductions and prints which levels the
machine let it compare — a Rust test, so this profile carries a toolchain.
It also runs everything else in `bench/`, because a 16-core x86 box is the
first honest multi-core measurement this project has had: every table in
bench/README.md was taken on a four-core 2017 laptop, and several of them
conclude "this is memory bandwidth" on a machine with two channels of
DDR4-2400.

**`graviton`.** The SIMD section says only that NEON is in the aarch64
baseline, so the two rungs there are *expected* to be the same code; this
measures whether they are. It also runs the Python suite against the wheel,
which is the only time that cross-compiled artifact is ever executed before
a user executes it. SVE is explicitly out of scope: the ladder has no rung
above the aarch64 baseline, so a Graviton3's 256-bit SVE is invisible to
libjay today. That rung is future work, and this run's
baseline-versus-native table is the evidence for how much it would be worth.

**`gpu`.** The run docs/decisions.md asks for by name. NVIDIA's Vulkan
driver reports `SHADER_F64`, so this is both the first f64 device
measurement and the validation that WGSL naga has only type-checked
computes what the CPU computes. The order of phases is deliberate: the
adapter report comes first and costs two minutes, so if the adapter turns
out not to have f64 after all, every later failure is already explained. A
Python equivalence check (four kernels, CPU f64 against GPU f64, asserting
1e-13) runs before `bench/device.py` and before the Rust battery, so the
headline answer lands early even if the four-vCPU cargo build later runs
out of time. The AMI is the Deep Learning Base OSS NVIDIA Driver image, so
no kernel module is built here; only the Vulkan loader is installed.

**`oracle`.** `record --check` re-measures every corpus expression against
jconsole and GNU APL as Linux runs them and fails on drift while writing
nothing — a gate. A full `record` follows into the same tree and the diff of
the snapshots is uploaded for the owner to read; nothing is committed by a
machine that is about to delete itself. The clean-room rule is unchanged:
both interpreters are black-box subprocesses, neither binary enters the
repository, and the jconsole tarball is fetched from Jsoftware each run
rather than mirrored. GNU APL is GPL source built on the instance — about
five minutes on 16 vCPU — and only the **build product** is cached to
`s3://<bucket>/prebuilt/gnu-apl-<version>-<arch>.tar.gz`, so the second run
of this profile skips the compile.

---

## The prebuild question

Five options were on the table.

**(a) Install a compiler and build on the instance.** Simplest, slowest,
and — the deciding objection — it measures a locally built artifact rather
than the one CI vouches for and users install, so the numbers would not be
comparable with anything. Kept as `--wheel none`, the escape hatch for a
tree with no CI run.

**(b) No compiler: the released wheel from PyPI, rivals from PyPI.**
manylinux wheels exist for linux x86_64 *and* aarch64, and polars, numpy and
numba all ship manylinux wheels for both, so a benchmark environment needs
no compiler at all. Works only for a tagged release.

**(c) For an unreleased tree: the CI artifact, relayed through S3.**
publish.yml's dry run already builds the full linux wheel matrix on every
`workflow_dispatch` and weekly on schedule. The instance has no GitHub
credential, so the **local** orchestrator downloads the artifact under the
owner's `gh` login and uploads it to the run's S3 prefix; the instance pulls
from S3 only. `launch.sh --wheel gh-latest` does exactly this, warns when
the artifact's head SHA is not the local HEAD, and pins the wheel by sha256
in the user-data.

**(d) A custom AMI.** It would save the four minutes of bootstrap, and it
costs an AMI lifecycle: build it, version it, patch it, remember which
profile uses which, and re-bake it whenever a dependency moves. At roughly
one run a week that is a bad trade — the saving is minutes, the burden is
permanent. Rejected. Reconsider if runs ever become frequent enough that
four minutes × N matters, or if the GPU driver install proves flaky.

**(e) Cross-compiling from the Mac** with maturin and zig. It works, and it
is unnecessary: CI already produces exactly these two linux wheels, on the
pinned toolchain, from a clean checkout, and a locally cross-built wheel
would be a third artifact nobody else ever sees. Rejected.

### Recommendation: (b) + (c), with a toolchain on two profiles

`--wheel pypi:X.Y.Z` for a released tag, `--wheel gh-latest` otherwise. Both
land the same kind of artifact — an abi3 manylinux wheel built by the pinned
1.89 toolchain in CI — so a cloud number and a user's experience are the
same build.

**One correction to the owner's framing, and it matters.** "No compiler on
target" holds for the *measuring* half and not for the *validating* half.
The AVX-512 equivalence battery is `tests/simd.rs` and the GPU f64 battery
is `tests/device.rs`; both are Rust tests, unreachable from a wheel. So
`avx512` and `gpu` install rustup and the pinned toolchain and build the
test binaries — about 10 minutes on 16 vCPU, longer on the GPU box's four.
The split is deliberate and visible in the phase list: the wheel produces
the numbers, the toolchain produces the verdicts, and a profile that fails
to build a toolchain still delivers every number it took before that phase.

Two smaller pieces follow the same shape. The **bench scripts, corpora and
Cargo manifests** travel as one `git archive` tarball of HEAD, pinned by
sha256 — no git, no GitHub credential, no working-tree contamination on the
instance. **GNU APL** is built once per architecture and its build product
cached in the bucket, which is the one place a build artifact is reused
across runs.

An open flaw worth naming: `gh run download` only reaches artifacts younger
than 90 days, and the weekly dry run keeps that satisfied only while the
schedule keeps running. If `gh-latest` finds nothing, `--wheel none` is the
fallback and the run says so in its log.

---

## How a run goes

```sh
bench/cloud/launch.sh avx512 --dry-run     # every check, launches nothing
bench/cloud/launch.sh avx512               # launch and watch
bench/cloud/launch.sh gpu --wheel pypi:0.2.0 --no-watch
bench/cloud/fetch-results.sh latest --tail
```

1. `launch.sh` runs the fourteen preflight checks, resolves the AMI from the
   public SSM parameter, picks the cheapest availability zone that both
   offers the type and has a subnet, stages the source tarball and the wheel
   into `s3://<bucket>/runs/<run-id>/input/`, renders the user-data
   (checking it is valid bash and under 16 KB), rehearses the launch with
   `--dry-run`, writes the ledger, and launches.
2. The instance arms three shutdown timers, brings up the log pump, records
   the machine, and runs its phases. Each phase's output goes to the console
   *and* to a file uploaded the moment the phase ends, so a run that dies in
   its fourth phase still delivers three.
3. `launch.sh` watches read-only: it polls `runs/<run-id>/STATUS` and the
   rolling `log/console.log` in S3 and prints new lines. Ctrl-C detaches
   without stopping anything. It cannot make the instance live longer, and
   the instance does not need it alive.
4. `finish()` writes a manifest, uploads everything, and powers off — which
   terminates.
5. `fetch-results.sh <run-id>` syncs the run to `bench/cloud/results/`,
   which is git-ignored. **A cloud number enters the repository only when
   the owner writes it into bench/README.md by hand**, with the provenance
   block those tables carry.

### Observability

Two channels, and the split is on purpose.

- **S3 is primary.** The full log is synced every 30 seconds to
  `runs/<run-id>/log/console.log`, a one-word `STATUS` object tracks the
  phase, and each phase's output lands as its own object. It needs nothing
  on the instance that is not already there for the upload, and it is what
  both `launch.sh --watch` and `fetch-results.sh` read.
- **CloudWatch Logs mirrors it**, one stream per run in `/libjay/bench`, via
  a small `PutLogEvents` pump that resumes from a byte offset. It is for
  the console and for a tail that does not poll an object. Every call to it
  is best-effort; a broken pump does not affect the run.

The CloudWatch *agent* was considered and not used: it is the standard
answer, but it needs an install that can itself fail before any channel
exists, and the S3 sync — which the run needs anyway — already covers the
same ground in three lines of shell. Sequence tokens are no longer required
by `PutLogEvents`, which is what makes the pump this small.

The pump also watches the spot interruption notice on IMDS and flushes
results the moment one appears; two minutes of warning is enough to save
whatever has been measured.

**No SSH and no SSM in v1.** No key pair is created, no ingress rule exists,
and nothing about the design assumes a human can log in. SSM Session Manager
is the documented later option: it needs `AmazonSSMManagedInstanceCore` on
the instance role and no inbound port, which is a modest and reversible
change if a run ever needs to be debugged interactively rather than
re-launched with more logging.

---

## Owner setup, once

Everything below is the owner's to run, from an administrative profile, and
none of it is in any script here. Substitute the real values for
`OWNER-ACCOUNT-ID`, `OWNER-REGION`, `OWNER-BUCKET-NAME`, `OWNER-VPC-ID`,
`OWNER-SECURITY-GROUP-ID` and `OWNER-ADMIN-USER` — `us-east-2` is the
suggested region (cheap, deep spot capacity for both c7i and g5, every AMI
used here published there), and the bucket name must be globally unique.

```sh
export ADM=--profile=admin                 # an administrative profile
export REGION=us-east-2
export ACCT=$(aws $ADM sts get-caller-identity --query Account --output text)
export BUCKET=libjay-bench-$ACCT
```

**1. The bucket.**

```sh
aws $ADM s3api create-bucket --bucket "$BUCKET" --region "$REGION" \
    --create-bucket-configuration LocationConstraint="$REGION"
aws $ADM s3api put-public-access-block --bucket "$BUCKET" \
    --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
aws $ADM s3api put-bucket-lifecycle-configuration --bucket "$BUCKET" \
    --lifecycle-configuration '{"Rules":[{"ID":"abort-mpu","Status":"Enabled",
      "Filter":{},"AbortIncompleteMultipartUpload":{"DaysAfterInitiation":7}}]}'
```

**2. The log group.** Pre-created, so the instance role never needs
`logs:CreateLogGroup`. Thirty days is plenty; the S3 copy is the archive.

```sh
aws $ADM logs create-log-group --region "$REGION" --log-group-name /libjay/bench
aws $ADM logs put-retention-policy --region "$REGION" \
    --log-group-name /libjay/bench --retention-in-days 30
```

**3. The security group.** No ingress at all; egress only to what a
bootstrap needs.

```sh
VPC=$(aws $ADM ec2 describe-vpcs --region "$REGION" \
      --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId' --output text)
SG=$(aws $ADM ec2 create-security-group --region "$REGION" \
     --group-name libjay-bench --description "libjay bench: no ingress" \
     --vpc-id "$VPC" --query GroupId --output text)
aws $ADM ec2 revoke-security-group-egress --region "$REGION" --group-id "$SG" \
    --protocol -1 --port -1 --cidr 0.0.0.0/0
for p in 80 443 53; do
  aws $ADM ec2 authorize-security-group-egress --region "$REGION" --group-id "$SG" \
      --protocol tcp --port $p --cidr 0.0.0.0/0
done
aws $ADM ec2 authorize-security-group-egress --region "$REGION" --group-id "$SG" \
    --protocol udp --port 53 --cidr 0.0.0.0/0
aws $ADM ec2 authorize-security-group-egress --region "$REGION" --group-id "$SG" \
    --protocol udp --port 123 --cidr 0.0.0.0/0
echo "VPC=$VPC SG=$SG"
```

**4. The instance role.** Substitute the placeholders in the policy files
first — `sed -i '' -e "s/OWNER-ACCOUNT-ID/$ACCT/g" -e "s/OWNER-REGION/$REGION/g"
-e "s/OWNER-BUCKET-NAME/$BUCKET/g" bench/cloud/policy/*.json`, and edit
`OWNER-SECURITY-GROUP-ID` and `OWNER-ADMIN-USER` by hand. **Do not commit
the substituted files**; work on a copy, or revert them afterwards.

```sh
aws $ADM iam create-role --role-name libjay-bench-instance \
    --assume-role-policy-document file://bench/cloud/policy/instance-trust.json
aws $ADM iam put-role-policy --role-name libjay-bench-instance \
    --policy-name libjay-bench-instance --policy-document \
    file://bench/cloud/policy/instance-role.json
aws $ADM iam create-instance-profile --instance-profile-name libjay-bench-instance
aws $ADM iam add-role-to-instance-profile \
    --instance-profile-name libjay-bench-instance --role-name libjay-bench-instance
```

**5. The launching user.** No console password, one inline policy, one key.

```sh
aws $ADM iam create-user --user-name libjay-bench-launcher
aws $ADM iam put-user-policy --user-name libjay-bench-launcher \
    --policy-name libjay-bench-launcher \
    --policy-document file://bench/cloud/policy/launcher.json
aws $ADM iam create-access-key --user-name libjay-bench-launcher
```

Put that key into `~/.aws/credentials` as `[libjay-bench]` and nowhere else.
Optionally lock the bucket to the two identities:

```sh
aws $ADM s3api put-bucket-policy --bucket "$BUCKET" \
    --policy file://bench/cloud/policy/bucket-policy.json
```

**6. The quotas — the step that bounds a thief.** Request a *decrease*, via
the Service Quotas console or CLI, for the region:

| quota | code | request |
|---|---|---|
| All Standard (A, C, D, H, I, M, R, T, Z) Spot Instance Requests | `L-34B43A08` | **16** vCPU |
| All G and VT Spot Instance Requests | `L-3819A6DF` | **4** vCPU |
| Running On-Demand Standard instances | `L-1216C47A` | **0** if the account runs nothing else |

```sh
aws $ADM service-quotas request-service-quota-increase --region "$REGION" \
    --service-code ec2 --quota-code L-34B43A08 --desired-value 16
```

A decrease is accepted through the same API and may take a day. If the
on-demand quota cannot go to zero because the account is shared, leave it —
the IAM `Deny` on non-spot launches already covers this user.

**7. The budget.** $20/month, alerting at 50%, 80% and 100% of *actual*, and
at 100% of *forecast*. The name must be `libjay-monthly`, which is what
`policy/launcher.json` grants `budgets:ViewBudget` on and what the spend
guard reads.

```sh
aws $ADM budgets create-budget --account-id "$ACCT" \
  --budget '{"BudgetName":"libjay-monthly","BudgetLimit":{"Amount":"20","Unit":"USD"},
             "TimeUnit":"MONTHLY","BudgetType":"COST"}' \
  --notifications-with-subscribers '[
    {"Notification":{"NotificationType":"ACTUAL","ComparisonOperator":"GREATER_THAN",
      "Threshold":50,"ThresholdType":"PERCENTAGE"},
     "Subscribers":[{"SubscriptionType":"EMAIL","Address":"OWNER-EMAIL"}]},
    {"Notification":{"NotificationType":"ACTUAL","ComparisonOperator":"GREATER_THAN",
      "Threshold":100,"ThresholdType":"PERCENTAGE"},
     "Subscribers":[{"SubscriptionType":"EMAIL","Address":"OWNER-EMAIL"}]},
    {"Notification":{"NotificationType":"FORECASTED","ComparisonOperator":"GREATER_THAN",
      "Threshold":100,"ThresholdType":"PERCENTAGE"},
     "Subscribers":[{"SubscriptionType":"EMAIL","Address":"OWNER-EMAIL"}]}]'
```

**8. Fill in `config.sh`** — `LIBJAY_ACCOUNT_ID`, `LIBJAY_REGION`,
`LIBJAY_BUCKET`, `LIBJAY_VPC_ID`, `LIBJAY_SECURITY_GROUP_ID` — or export
them. Until then every script here refuses to start.

**9. Rehearse.** `bench/cloud/launch.sh avx512 --dry-run` exercises every
check and the EC2 permission rehearsal, and launches nothing. It should
print a plan and a max-spend bound and exit 0.

---

## Open questions for the owner

1. **Region.** `us-east-2` is the suggestion. The IAM policy pins whichever
   is chosen, and moving it later means editing the policy and re-uploading
   the prebuilt cache.
2. **`g5.2xlarge` in the type allowlist?** The GPU profile's long pole is
   building the Rust test binaries on four vCPU. Eight vCPU would roughly
   halve it at about the same total cost, since the run gets shorter. It
   widens the type allowlist by one entry.
3. **The spend guard threshold**, `$15` against a `$20` budget, and whether
   a refusal or a warning is wanted when the Budget cannot be read at all
   (today: it proceeds on the ledger alone and says so).
4. **Mirror apt and PyPI into the bucket?** It would let the security group
   drop to S3-endpoint-only egress and make a stolen key nearly worthless,
   at the cost of maintaining a mirror.
5. **SSM in v2?** One managed policy on the instance role, no inbound port.
   Worth it the first time a GPU run fails for a reason the log does not
   explain.
6. **Dyalog is not here.** The licence click-through is the owner's alone,
   so a Dyalog oracle cannot be installed by an unattended instance. The
   path if it is ever wanted: accept the licence once on the Mac, put the
   installer in `prebuilt/` under the owner's own credentials, and have the
   oracle profile install from there — the run-only quarantine and the
   `--impl dyalog` recording path already exist.
7. **What happens to the numbers.** These runs produce tables for
   bench/README.md's provenance style, on machines nobody can re-run
   cheaply. Whether a cloud table lives in bench/README.md beside the laptop
   ones, or in a file of its own, is a documentation decision worth making
   before the first run lands.

---

## Later

- **SVE.** A rung above the aarch64 baseline; the `graviton` profile's
  baseline-versus-native table is the evidence for whether it pays.
- **The oracle cache, wider.** jconsole is fetched from Jsoftware each run
  rather than mirrored, on purpose — it is not ours to redistribute, even
  privately. If the download ever becomes the flaky part, the question is
  worth revisiting with the licence in hand.
- **Windows/DX12.** The other adapter family with f64 in shaders. Nothing in
  this directory is Windows-shaped; it would be a fifth profile and a
  different bootstrap language.
