// Command go_driver_smoke is the official Go driver smoke test against
// `relate serve`'s Bolt surface — the Go analog of
// `../python_driver_smoke.py`. It pins the LABS-598 finding: none of the
// neo4j-go-driver's Bolt 5 handshake extras — the manifest-marker
// proposal (`00 00 01 FF`) or the `patch_bolt:["utc"]` HELLO extra —
// break Relate's Bolt 4.4 path, so a backend serving both Relate
// and a Neo4j 5.x runs the same binary and the unmodified driver config
// (default pool, default timeouts) against both engines.
//
// The driver is constructed with no config functions at all: any
// session-level compat rule (LABS-595's never-set notification
// filtering) is the default `SessionConfig`, never an explicit setting.
//
// The topology's one source is scripts/bolt-smoke.sh, which exports every
// fact this smoke reads: RELATE_BOLT_HOST, RELATE_BOLT_PORT_DEFAULT
// (the plaintext server), and RELATE_BOLT_PORT_PRESET_AUTH (the
// preset-auth server). Run through the script, or export the variables
// beside your own servers started the same way:
//
//	export RELATE_BOLT_HOST=127.0.0.1 \
//	  RELATE_BOLT_PORT_DEFAULT=7687 RELATE_BOLT_PORT_PRESET_AUTH=7689
//	cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --auth none &
//	cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7689 --database :memory: --auth relate:ci-token &
//	go run ./surfaces/relate-bolt/tests/go_driver_smoke
//
// Every topology fact is required — unset or blank is a configuration
// error, never a silent default and never a skip, matching the sibling
// smokes' gates.
package main

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/neo4j/neo4j-go-driver/v5/neo4j"
)

// The one property shared-code engine detection relies on (LABS-597):
// the HELLO SUCCESS agent string starts with `Relate/` (ADR-0293).
const agentPrefix = "Relate/"

// requiredEnv names a fact the topology owns: unset or blank is a
// configuration error, never a silent default.
func requiredEnv(name string) (string, error) {
	value := os.Getenv(name)
	if strings.TrimSpace(value) == "" {
		return "", fmt.Errorf("%s must be set — run via scripts/bolt-smoke.sh "+
			"or export the topology beside your own servers", name)
	}
	return value, nil
}

// boltUri resolves the named port fact and the host fact from the
// topology's one source and joins them — one call, one error check.
func boltUri(name string) (string, error) {
	host, err := requiredEnv("RELATE_BOLT_HOST")
	if err != nil {
		return "", err
	}
	port, err := requiredEnv(name)
	if err != nil {
		return "", err
	}
	return "bolt://" + host + ":" + port, nil
}

func main() {
	ctx := context.Background()
	if err := run(ctx); err != nil {
		fmt.Fprintln(os.Stderr, "go driver smoke: FAILED:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context) error {
	if err := plaintextSmoke(ctx); err != nil {
		return err
	}
	return authSmoke(ctx)
}

// plaintextSmoke connects to the `--auth none` server the topology names
// with the unmodified driver config and asserts the query and the
// detection probe.
func plaintextSmoke(ctx context.Context) error {
	uri, err := boltUri("RELATE_BOLT_PORT_DEFAULT")
	if err != nil {
		return err
	}
	driver, err := neo4j.NewDriverWithContext(uri, neo4j.NoAuth())
	if err != nil {
		return fmt.Errorf("constructing the driver: %w", err)
	}
	defer func() { _ = driver.Close(ctx) }()

	agent, err := returnOne(ctx, driver)
	if err != nil {
		return err
	}
	if !strings.HasPrefix(agent, agentPrefix) {
		return fmt.Errorf("ServerInfo.Agent %q does not start with %q", agent, agentPrefix)
	}
	fmt.Printf("plaintext smoke: RETURN 1 ran, agent %q\n", agent)
	return nil
}

// authSmoke authenticates with BasicAuth against the preset-credential
// server (`--auth relate:ci-token`) — a required topology fact like
// every other: unset is a configuration error, not a skip. The python
// smoke's auth section behaves the same way.
func authSmoke(ctx context.Context) error {
	uri, err := boltUri("RELATE_BOLT_PORT_PRESET_AUTH")
	if err != nil {
		return err
	}
	driver, err := neo4j.NewDriverWithContext(uri, neo4j.BasicAuth("relate", "ci-token", ""))
	if err != nil {
		return fmt.Errorf("constructing the driver: %w", err)
	}
	defer func() { _ = driver.Close(ctx) }()

	if _, err := returnOne(ctx, driver); err != nil {
		return err
	}
	fmt.Println("auth smoke: BasicAuth accepted, RETURN 1 ran")
	return nil
}

// returnOne runs `RETURN 1 AS n` through the driver's managed
// ExecuteQuery — the retry-managed path LABS-596 names as the shared
// backend's one retry policy — and returns the server's agent string.
func returnOne(ctx context.Context, driver neo4j.DriverWithContext) (string, error) {
	result, err := neo4j.ExecuteQuery(ctx, driver, "RETURN 1 AS n", nil, neo4j.EagerResultTransformer)
	if err != nil {
		return "", fmt.Errorf("ExecuteQuery RETURN 1: %w", err)
	}
	if len(result.Records) != 1 {
		return "", fmt.Errorf("expected 1 record, got %d", len(result.Records))
	}
	n, ok := result.Records[0].Get("n")
	if !ok || n != int64(1) {
		return "", fmt.Errorf("expected n == 1, got %v", n)
	}
	agent := result.Summary.Server().Agent()
	if agent == "" {
		return "", fmt.Errorf("empty ServerInfo.Agent")
	}
	return agent, nil
}
