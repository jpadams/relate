package com.relate.smoke;

import java.util.Optional;

import org.neo4j.driver.AuthTokens;
import org.neo4j.driver.Driver;
import org.neo4j.driver.GraphDatabase;
import org.neo4j.driver.Record;
import org.neo4j.driver.Result;
import org.neo4j.driver.Session;
import org.neo4j.driver.Transaction;
import org.neo4j.driver.Values;
import org.neo4j.driver.exceptions.Neo4jException;
import org.neo4j.driver.exceptions.TransientException;
import org.neo4j.driver.exceptions.UntrustedServerException;

/**
 * The official Java driver smoke test against {@code relate serve}'s
 * Bolt surface — the Java analog of {@code ../go_driver_smoke} and
 * {@code ../python_driver_smoke.py}.
 *
 * <p>It pins the transport premise the Jepsen harness depends on (LABS-649),
 * including the one break the other smokes cannot show: since 5.0 the
 * neo4j-java-driver validates the HELLO SUCCESS {@code server} agent and
 * throws {@link UntrustedServerException} unless it starts with
 * {@code Neo4j/} — the default {@code Relate/<version>} identity
 * (ADR-0293) is rejected. The remedy the CLI ships for exactly this
 * (spec: cypher-shell precedent) is {@code --bolt-identity}: a server
 * started with a Neo4j-shaped identity admits the driver verbatim. The
 * smoke asserts both sides.
 *
 * <p>All three servers are required — a missing one fails loudly as a
 * connection error, never a skipped section: every topology fact is
 * required across all the driver smokes now, and this smoke's core
 * section is the identity path itself, where a silent skip would print
 * {@code ok} without exercising anything. The whole server topology —
 * identity, ports, host — is owned by {@code scripts/bolt-smoke.sh},
 * exported as {@code RELATE_BOLT_IDENTITY},
 * {@code RELATE_BOLT_PORT_DEFAULT},
 * {@code RELATE_BOLT_PORT_IDENTITY},
 * {@code RELATE_BOLT_PORT_AUTH_IDENTITY} and
 * {@code RELATE_BOLT_HOST} — the one source for every fact, so
 * script/smoke drift is structurally impossible, and every required
 * value fails loudly when absent.
 *
 * <p>The write-write conflict section asserts the mapping the Jepsen Elle
 * lane must observe unmasked: a conflicting second committer surfaces
 * {@code Neo.TransientError.Transaction.Outdated} as a
 * {@link TransientException} (ADR-0015 first-committer-wins, ADR-0017 the
 * engine never retries, spec table in {@code docs/api/bolt/spec.md}). The
 * driver's managed {@code executeWrite} retries exactly that
 * classification, which would silently mask the conflict — so this smoke,
 * like the future Jepsen client, uses explicit transactions only.
 *
 * <p>Start the servers first (the CI smoke leg does exactly this):
 *
 * <pre>
 *   export RELATE_BOLT_IDENTITY=Neo4j/5.26.0
 *   export RELATE_BOLT_PORT_DEFAULT=7687 RELATE_BOLT_PORT_IDENTITY=7691 \
          RELATE_BOLT_PORT_AUTH_IDENTITY=7692 RELATE_BOLT_HOST=127.0.0.1
 *   cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --auth none &amp;
 *   cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7691 --database :memory: --auth none --bolt-identity "$RELATE_BOLT_IDENTITY" &amp;
 *   cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7692 --database :memory: --auth relate:ci-token --bolt-identity "$RELATE_BOLT_IDENTITY" &amp;
 *   (cd surfaces/relate-bolt/tests/java_driver_smoke &amp;&amp; mvn -q -B compile exec:java)
 * </pre>
 *
 * <p>{@code RELATE_BOLT_HOST} is a required topology fact like every
 * other — unset or blank fails loudly at startup, before any connection
 * attempt. A containerized maven run exports it pointing at
 * {@code host.docker.internal}, exactly as the start recipe above
 * exports it for a local run.
 */
public final class JavaDriverSmoke {
    private static final String OUTDATED_CODE = "Neo.TransientError.Transaction.Outdated";

    private static final String UNAUTHORIZED_CODE = "Neo.ClientError.Security.Unauthorized";

    private JavaDriverSmoke() {}

    /** The resolved server topology, every fact from the one source
     * (bolt-smoke.sh's exports), failing loudly on anything absent. */
    private record Config(String host, int defaultPort, int identityPort, int authIdentityPort,
                          String identity) {
        static Config fromEnv() {
            return new Config(
                    requiredEnv("RELATE_BOLT_HOST", "the servers' host"),
                    requiredPort("RELATE_BOLT_PORT_DEFAULT",
                            "the default-identity server"),
                    requiredPort("RELATE_BOLT_PORT_IDENTITY",
                            "the --bolt-identity server"),
                    requiredPort("RELATE_BOLT_PORT_AUTH_IDENTITY",
                            "the Java smoke's auth server (preset auth behind the --bolt-identity shim)"),
                    requiredEnv("RELATE_BOLT_IDENTITY",
                            "the --bolt-identity the servers were started with"));
        }

        /** The one spelling of the bolt:// URI join. */
        String boltUri(int port) {
            return "bolt://" + host + ":" + port;
        }
    }

    public static void main(String[] args) {
        try {
            // Configuration faults precede any connection attempt.
            Config config = Config.fromEnv();
            identityRejectionSmoke(config);
            plaintextSmoke(config);
            authSmoke(config);
        } catch (Exception e) {
            System.err.println("java driver smoke: FAILED: " + e);
            System.exit(1);
        }
        System.out.println("ok");
    }

    /** The LABS-649 finding, pinned: driver 5.x refuses the honest
     * {@code Relate/<version>} agent — the break the {@code
     * --bolt-identity} shim exists for. */
    private static void identityRejectionSmoke(Config config) {
        UntrustedServerException e = expectRejected(() -> {
            try (Driver driver = GraphDatabase.driver(
                    config.boltUri(config.defaultPort()), AuthTokens.none());
                 Session session = driver.session()) {
                session.run("RETURN 1 AS n").consume();
            }
        }, UntrustedServerException.class);
        // UntrustedServerException exposes no typed status code — the
        // driver's message text is the pinned contract for this probe.
        check(String.valueOf(e).contains("genuine Neo4j instance"),
                "expected the genuine-Neo4j rejection, got %s", e);
        System.out.println("identity rejection smoke: driver 5.x rejects the "
                + "default Relate/ agent (the --bolt-identity use case)");
    }

    /** The full suite against the {@code --bolt-identity} server: the 5.x
     * handshake, verbatim identity, params, explicit transactions, and
     * the conflict mapping. */
    private static void plaintextSmoke(Config config) {
        try (Driver driver = GraphDatabase.driver(
                config.boltUri(config.identityPort()), AuthTokens.none())) {
            String agent;
            try (Session session = driver.session()) {
                Result result = session.run("RETURN 1 AS n");
                Record record = result.next();
                check(record.get("n").asInt() == 1, "RETURN 1 must yield 1");
                agent = result.consume().server().agent();
            }
            check(config.identity().equals(agent),
                    "the --bolt-identity value must be sent verbatim, got %s", agent);

            parameterSmoke(driver);
            explicitTransactionSmoke(driver);
            conflictSmoke(driver);
        }
        System.out.println("plaintext smoke: RETURN 1, verbatim identity, "
                + "params, explicit transactions, conflict mapping all ran");
    }

    /** Parameterized writes and reads through the simple query API — the
     * read filters on its bound parameter, so the read path exercises
     * real parameter binding, not just an unused slot. */
    private static void parameterSmoke(Driver driver) {
        try (Session session = driver.session()) {
            session.run("MERGE (p:Person {name: $name})", Values.parameters("name", "Ada"))
                    .consume();
            Record record = session.run(
                    "MATCH (p:Person {name: $name}) RETURN p.name AS name",
                    Values.parameters("name", "Ada"))
                    .single();
            check("Ada".equals(record.get("name").asString()),
                    "expected the seeded name back through the bound parameter");
        }
    }

    /** Explicit BEGIN/COMMIT with read-your-own-write inside the
     * transaction, plus ROLLBACK discarding the write — the shape the
     * Jepsen bank/counter lanes drive. */
    private static void explicitTransactionSmoke(Driver driver) {
        try (Session session = driver.session()) {
            try (Transaction tx = session.beginTransaction()) {
                tx.run("MERGE (n:Txn {name: $name})", Values.parameters("name", "Bob"))
                        .consume();
                Record own = tx.run("MATCH (n:Txn) RETURN n.name AS name").single();
                check("Bob".equals(own.get("name").asString()),
                        "a transaction must read its own writes");
                tx.commit();
            }
            Record committed = session.run("MATCH (n:Txn) RETURN n.name AS name").single();
            check("Bob".equals(committed.get("name").asString()),
                    "the committed write must be visible");

            try (Transaction tx = session.beginTransaction()) {
                tx.run("MERGE (n:Txn {name: $name})", Values.parameters("name", "Zoe"))
                        .consume();
                tx.rollback();
            }
            long count = session.run("MATCH (n:Txn) RETURN count(n) AS c").single()
                    .get("c").asLong();
            check(count == 1, "a rolled-back write must leave exactly the "
                    + "committed row, got count=%d", count);
        }
    }

    /** Runs one conflict-prone step, returning the TransientException it
     * raised (null when the step succeeded) — the one catch site for the
     * whole section. Any other failure propagates. */
    private static TransientException attempt(Runnable step) {
        try {
            step.run();
            return null;
        } catch (TransientException e) {
            return e;
        }
    }

    /** Two explicit transactions writing the same node: the first
     * committer wins; the second surfaces the Outdated TransientException
     * (ADR-0015/ADR-0017). The exception may arrive at the write statement
     * or at COMMIT — whichever point the engine appends to the WAL — so
     * both steps accept it, and the final value must be the winner's. The
     * gate between the two attempts is load-bearing: when the conflict
     * fires at the loser's write, the winner's write and commit must still
     * run, or the final-value assertion would judge the seed. */
    private static void conflictSmoke(Driver driver) {
        try (Session session = driver.session()) {
            session.run("MERGE (c:Counter {k: 'conflict'}) SET c.v = 0").consume();
        }
        TransientException conflict;
        try (Session winner = driver.session();
             Session loser = driver.session();
             Transaction first = winner.beginTransaction();
             Transaction second = loser.beginTransaction()) {
            conflict = attempt(() ->
                    second.run("MATCH (c:Counter {k: 'conflict'}) SET c.v = 2")
                            .consume());
            first.run("MATCH (c:Counter {k: 'conflict'}) SET c.v = 1").consume();
            first.commit();
            if (conflict == null) {
                conflict = attempt(second::commit);
            }
        }
        check(conflict != null,
                "the second committer must conflict, but neither its write "
                        + "nor its COMMIT raised the error");
        check(OUTDATED_CODE.equals(conflict.code()),
                "the conflict code must be %s, got %s", OUTDATED_CODE, conflict.code());
        try (Session session = driver.session()) {
            long v = session.run("MATCH (c:Counter {k: 'conflict'}) RETURN c.v AS v")
                    .single().get("v").asLong();
            check(v == 1, "the first committer's value must win, got v=%d", v);
        }
        System.out.println("conflict smoke: second committer got "
                + OUTDATED_CODE + " (the classification managed "
                + "executeWrite would auto-retry — hence explicit-only)");
    }

    /** Preset-credential server (ADR-0126) behind the same {@code
     * --bolt-identity} shim, started by the CI smoke leg with {@code
     * --auth relate:ci-token}. The credential pair is a fixture
     * shared by name with the sibling smokes and the script's spawns; a
     * drift fails loudly in CI. */
    private static void authSmoke(Config config) {
        try (Driver driver = GraphDatabase.driver(
                config.boltUri(config.authIdentityPort()),
                AuthTokens.basic("relate", "ci-token"))) {
            try (Session session = driver.session()) {
                long n = session.run("RETURN 1 AS n").single().get("n").asLong();
                check(n == 1, "RETURN 1 with preset credentials must yield 1");
            }
        }
        Neo4jException e = expectRejected(() -> {
            try (Driver driver = GraphDatabase.driver(
                    config.boltUri(config.authIdentityPort()),
                    AuthTokens.basic("relate", "wrong"));
                 Session session = driver.session()) {
                session.run("RETURN 1 AS n").consume();
            }
        }, Neo4jException.class);
        // The FAILURE carries the Neo4j status code with the engine's
        // message text ("authentication failed", session.rs) — the
        // code is the stable contract, so pin that.
        check(UNAUTHORIZED_CODE.equals(e.code()),
                "expected the Unauthorized status code, got %s", e.code());
        System.out.println("auth smoke: preset accepted, wrong credentials rejected");
    }

    /** The one absence rule: a null or blank env var counts as absent. */
    private static Optional<String> env(String name) {
        String value = System.getenv(name);
        return value == null || value.isBlank() ? Optional.empty() : Optional.of(value);
    }

    /** Loud on absence: a required configuration fact never silently
     * defaults — bolt-smoke.sh owns the value and exports it. */
    private static String requiredEnv(String name, String what) {
        return env(name).orElseThrow(() -> new IllegalStateException(
                name + " must be set: " + what));
    }

    private static int requiredPort(String name, String what) {
        String value = requiredEnv(name, what);
        try {
            return Integer.parseInt(value);
        } catch (NumberFormatException e) {
            throw new IllegalStateException(
                    name + " must be a port number, got " + value);
        }
    }

    /** Runs a probe that must be rejected, failing loudly when it is not,
     * and returns the typed rejection — the one spelling of expect-throw,
     * so a negative assertion never hides inside the catch that is its
     * own success signal. Any other failure propagates. */
    private static <E extends RuntimeException> E expectRejected(
            Runnable probe, Class<E> type) {
        try {
            probe.run();
        } catch (RuntimeException e) {
            if (!type.isInstance(e)) {
                throw e;
            }
            return type.cast(e);
        }
        throw new IllegalStateException(
                "expected the probe to be rejected with " + type.getSimpleName()
                        + ", but it succeeded");
    }

    private static void check(boolean condition, String message, Object... args) {
        if (!condition) {
            throw new IllegalStateException(String.format(message, args));
        }
    }
}
