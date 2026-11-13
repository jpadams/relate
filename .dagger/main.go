// Relate's merge gates as Dagger checks.
//
// Each `+check` function is one gate from AGENTS.md "Final gates", run as
// `dagger check` locally and by Dagger Cloud Checks on every push and pull
// request. A check is a pure function of the source it reads, so an
// unchanged input is a cache hit: the gate replays its recorded result
// instead of recompiling.
package relate_ci

import (
	"context"
	"strconv"
	"strings"

	"dagger.io/dagger/core"

	"dagger/relate-ci/internal/dagger"
)

// The baseline build environment (ADR-0070): the image the baseline
// loadtests compile in, on the toolchain rust-toolchain.toml pins.
const rustImage = "rust:1.88-bookworm"

// The test runner CI uses (.sonic/docker/sonic-rust.Dockerfile), same pin.
// nextest runs each test in its own process, as the CI lanes do.
const nextestVersion = "0.9.87"

type RelateCi struct {
	// +private
	Source *dagger.Directory
	// +private
	Jobs int
}

func New(
	ws *dagger.Workspace,
	// Cap on cargo's parallel compile and link jobs; 0 keeps cargo's
	// default (one per CPU). Lower it on a memory-tight local engine:
	// `dagger settings --global relate-ci jobs 4`.
	// +default=0
	jobs int,
) *RelateCi {
	return &RelateCi{
		Jobs: jobs,
		// Only tracked inputs reach a check: .gitignore drops target/ and
		// run output, and the CI module itself is excluded so editing the
		// pipeline does not invalidate every gate's cached result.
		Source: ws.Directory("/", dagger.WorkspaceDirectoryOpts{
			Gitignore: true,
			Exclude:   []string{".git", ".dagger", ".claude", "dagger.toml", "dagger.lock"},
		}),
	}
}

// Rustfmt over the whole workspace.
//
// +check
func (m *RelateCi) Fmt(ctx context.Context) error {
	return m.cargo(ctx, "fmt", "fmt --all --check")
}

// Workspace clippy, every target, warnings denied.
//
// +check
func (m *RelateCi) Clippy(ctx context.Context) error {
	return m.cargo(ctx, "clippy", "clippy --workspace --all-targets --exclude relate-python -- -D warnings")
}

// Clippy on the CLI's agent bundle.
//
// +check
func (m *RelateCi) ClippyAgent(ctx context.Context) error {
	return m.cargo(ctx, "clippy-agent", "clippy -p relate-cli --all-targets --no-default-features --features agent -- -D warnings")
}

// Clippy on the CLI's minimal bundle.
//
// +check
func (m *RelateCi) ClippyMinimal(ctx context.Context) error {
	return m.cargo(ctx, "clippy-minimal", "clippy -p relate-cli --all-targets --no-default-features --features minimal -- -D warnings")
}

// Clippy on the CLI's bare serve bundle.
//
// +check
func (m *RelateCi) ClippyServe(ctx context.Context) error {
	return m.cargo(ctx, "clippy-serve", "clippy -p relate-cli --all-targets --no-default-features --features serve -- -D warnings")
}

// Crate-layout dependency rules (ADR-0103).
//
// +check
func (m *RelateCi) Boundaries(ctx context.Context) error {
	return m.cargo(ctx, "boundaries", "xtask check-boundaries")
}

// The CLI suite on the full (default) bundle.
//
// +check
func (m *RelateCi) TestCli(ctx context.Context) error {
	return m.cargo(ctx, "test-cli", "nextest run -p relate-cli")
}

// The CLI suite on the agent bundle.
//
// +check
func (m *RelateCi) TestCliAgent(ctx context.Context) error {
	return m.cargo(ctx, "test-cli-agent", "nextest run -p relate-cli --no-default-features --features agent")
}

// The CLI suite on the minimal bundle.
//
// +check
func (m *RelateCi) TestCliMinimal(ctx context.Context) error {
	return m.cargo(ctx, "test-cli-minimal", "nextest run -p relate-cli --no-default-features --features minimal")
}

// The CLI suite on the bare serve bundle.
//
// +check
func (m *RelateCi) TestCliServe(ctx context.Context) error {
	return m.cargo(ctx, "test-cli-serve", "nextest run -p relate-cli --no-default-features --features serve")
}

// The Neo4j/Aura loader suite.
//
// +check
func (m *RelateCi) TestLoad(ctx context.Context) error {
	return m.cargo(ctx, "test-load", "nextest run -p relate-load", "test --doc -p relate-load")
}

// Runs cargo invocations, in order, over the source. Each gate owns its
// target directory: separate volumes keep cargo's build-dir lock from
// serializing parallel gates and keep one feature set's artifacts from
// evicting another's. The registry is shared and read-mostly.
func (m *RelateCi) cargo(ctx context.Context, gate string, invocations ...string) error {
	ctr := m.toolchain().
		WithMountedCache("/usr/local/cargo/registry", core.NewCacheVolume("relate-cargo-registry")).
		WithMountedCache("/usr/local/cargo/git", core.NewCacheVolume("relate-cargo-git")).
		// LOCKED: two runs of one gate (two commits at once on Cloud)
		// take turns on its build tree and mtime ledger.
		WithMountedCache("/target", core.NewCacheVolume("relate-target-"+gate),
			dagger.ContainerWithMountedCacheOpts{Sharing: dagger.CacheSharingModeLocked}).
		WithEnvVariable("CARGO_TARGET_DIR", "/target").
		// ci.yml's workflow env: one-shot builds, trimmed debug info, and
		// the CLI's usage metrics opted out (ADR-0079).
		WithEnvVariable("CARGO_INCREMENTAL", "0").
		WithEnvVariable("CARGO_PROFILE_DEV_DEBUG", "line-tables-only").
		WithEnvVariable("DO_NOT_TRACK", "1")
	if m.Jobs > 0 {
		ctr = ctr.WithEnvVariable("CARGO_BUILD_JOBS", strconv.Itoa(m.Jobs))
	}
	ctr = ctr.WithDirectory("/src", m.Source).
		WithWorkdir("/src").
		WithExec([]string{"perl", "-e", stampMtimes})
	for _, args := range invocations {
		ctr = ctr.WithExec(append([]string{"cargo"}, strings.Fields(args)...))
	}
	_, err := ctr.Sync(ctx)
	return err
}

// Cargo decides freshness by mtime alone: a source newer than its build
// output is rebuilt. Source loaded from a git ref carries one fixed mtime
// (1980), older than every artifact in a warm target volume, so a changed
// file would look fresh and the gate would pass on stale artifacts. This
// gives each file the mtime a developer's checkout would: a ledger in the
// gate's target volume records each path's content hash and stamp; an
// unchanged file keeps its stamp, a new or changed one (a revert included)
// is stamped now. The ledger is written before cargo runs, as a working
// tree's mtimes change before a build, so a failed build leaves the same
// state a failed local build does.
const stampMtimes = `
use strict; use warnings; use Digest::SHA; use File::Find;
my $ledger = "/target/.source-mtimes";
my %seen;
if (open my $in, "<", $ledger) {
    while (<$in>) { chomp; my ($hash, $stamp, $path) = split / /, $_, 3; $seen{$path} = [$hash, $stamp]; }
}
my $now = time;
open my $out, ">", "$ledger.tmp" or die "ledger: $!";
find({ no_chdir => 1, wanted => sub {
    return unless -f $_ && !-l $_;
    my $hash = Digest::SHA->new(256)->addfile($_)->hexdigest;
    my $prev = $seen{$_};
    my $stamp = ($prev && $prev->[0] eq $hash) ? $prev->[1] : $now;
    utime $stamp, $stamp, $_ or die "utime $_: $!";
    print $out "$hash $stamp $_\n";
}}, ".");
close $out or die "ledger: $!";
rename "$ledger.tmp", $ledger or die "ledger: $!";
`

// The Rust image plus the components and test runner the gates use. It
// reads no source, so every gate shares one cached copy.
func (m *RelateCi) toolchain() *dagger.Container {
	release := "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-" + nextestVersion
	asset := "cargo-nextest-" + nextestVersion + "-$(uname -m)-unknown-linux-gnu"
	return core.NewContainer().
		From(rustImage).
		WithExec([]string{"rustup", "component", "add", "rustfmt", "clippy"}).
		WithExec([]string{"sh", "-euc", `
			curl -fsSL -o /tmp/nextest.tar.gz "` + release + `/` + asset + `.tar.gz"
			curl -fsSL -o /tmp/nextest.sha256 "` + release + `/` + asset + `.sha256"
			[ "$(awk '{print $1; exit}' /tmp/nextest.sha256)" = "$(sha256sum /tmp/nextest.tar.gz | awk '{print $1}')" ]
			mkdir /tmp/nextest && tar -xzf /tmp/nextest.tar.gz -C /tmp/nextest
			install -m 0755 "$(find /tmp/nextest -type f -name cargo-nextest | head -n 1)" /usr/local/cargo/bin/
			rm -rf /tmp/nextest /tmp/nextest.tar.gz /tmp/nextest.sha256
			cargo nextest --version`})
}
