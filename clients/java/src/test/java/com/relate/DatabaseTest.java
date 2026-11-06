package com.relate;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

public class DatabaseTest {
    @Test
    void returnOne() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        Database.Result r = conn.query("RETURN 1 AS n");
        assertEquals("n", r.columns().get(0));
        assertEquals(1, ((Number) r.rows().get(0).get(0)).intValue());
    }

    @Test
    void explicitTransaction() {
        Database db = Database.open(":memory:");
        Database.Connection w = db.connect();
        Database.Connection r = db.connect();
        w.begin();
        w.query("CREATE (n:Person {name: 'Ada'})");
        assertEquals(0, ((Number) r.query("MATCH (n:Person) RETURN count(n) AS n").rows().get(0).get(0)).intValue());
        w.commit();
        assertEquals("Ada", r.query("MATCH (n:Person) RETURN n.name AS name").rows().get(0).get(0));
    }

    @Test
    void connectWithNamespace() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect("system");
        conn.setNamespace("neo4j");
        Database.Result r = conn.query("RETURN 1 AS n");
        assertEquals(1, ((Number) r.rows().get(0).get(0)).intValue());
    }

    @Test
    void connectWithUnknownNamespace() {
        Database db = Database.open(":memory:");
        RelateException e = assertThrows(RelateException.class, () -> db.connect("nope"));
        assertTrue(e.getMessage().endsWith("Graph not found: nope"),
                "unexpected message: " + e.getMessage());
    }

    @Test
    void setNamespaceCaseInsensitive() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        conn.setNamespace("SYSTEM");
        conn.setNamespace("System_Ontologies");
        conn.setNamespace("neo4j");
        assertEquals(1, ((Number) conn.query("RETURN 1 AS n").rows().get(0).get(0)).intValue());
    }

    @Test
    void setNamespaceUnknownName() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        RelateException e = assertThrows(RelateException.class, () -> conn.setNamespace("nope"));
        assertTrue(e.getMessage().endsWith("Graph not found: nope"),
                "unexpected message: " + e.getMessage());
    }

    @Test
    void explainCreateReportsWriteTypeWithoutExecuting() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        Database.Result r = conn.query("EXPLAIN CREATE (n)");
        assertEquals("w", r.getQueryType());
        // EXPLAIN must not have executed the CREATE.
        assertEquals(0, conn.query("MATCH (n) RETURN n").rows().size());
    }

    @Test
    void useRoutedStatementReportsResolvedNamespace() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        Database.Result routed = conn.query("USE system SHOW DATABASES YIELD name RETURN name");
        assertEquals("system", routed.getNamespace());
        // The session namespace is unchanged: an unrouted statement still
        // resolves to neo4j.
        Database.Result unrouted = conn.query("RETURN 1 AS n");
        assertEquals("neo4j", unrouted.getNamespace());
    }

    @Test
    void setNamespaceRefusedInTransaction() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        conn.begin();
        try {
            RelateException e = assertThrows(RelateException.class, () -> conn.setNamespace("system"));
            assertTrue(e.getMessage().endsWith("Cannot change the namespace while a transaction is open."),
                    "unexpected message: " + e.getMessage());
        } finally {
            conn.rollback();
        }
        conn.setNamespace("system");
    }

    @Test
    void readOnlyRejectsWrites() {
        Database db = Database.openReadOnly(":memory:");
        Database.Connection conn = db.connect();
        assertEquals(1, ((Number) conn.query("RETURN 1 AS n").rows().get(0).get(0)).intValue());
        RelateException e = assertThrows(RelateException.class, () -> conn.query("CREATE ()"));
        assertTrue(e.getMessage().contains("read-only"), e.getMessage());
        Database.open(":memory:").connect().query("CREATE ()");
    }

    @Test
    void explainReportsCartesianProductNotification() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        conn.query("CREATE (:Person)");
        conn.query("CREATE (:City)");

        Database.Result explained = conn.query("EXPLAIN MATCH (a:Person), (b:City) RETURN a, b");
        Notification cartesian = explained.getNotifications().stream()
                .filter(n -> "Neo.ClientNotification.Statement.CartesianProduct".equals(n.getCode()))
                .findFirst()
                .orElseThrow(() -> new AssertionError(
                        "expected a CartesianProduct notification, saw " + explained.getNotifications()));
        assertEquals("03N90", cartesian.getGqlStatus());
        assertEquals("INFORMATION", cartesian.getSeverity());
        assertEquals("PERFORMANCE", cartesian.getCategory());
        assertEquals("Cartesian product", cartesian.getTitle());
        assertFalse(cartesian.getDescription().isEmpty());
        assertFalse(cartesian.getRemedy().isEmpty());
        assertNotNull(cartesian.getOperatorId());

        // A plain run derives no notifications: the list is present and empty.
        assertTrue(conn.query("RETURN 1 AS n").getNotifications().isEmpty());
    }

    @Test
    void explainReportsThePlanDocument() {
        Database db = Database.open(":memory:");
        Database.Connection conn = db.connect();
        conn.query("CREATE (:Person)");
        conn.query("CREATE (:City)");

        PlanDocument plan = conn.query("EXPLAIN MATCH (p:Person) RETURN p").getPlan();
        assertNotNull(plan, "EXPLAIN carries a plan document");
        assertEquals("relate.plan/v1", plan.schema);
        assertFalse(plan.profiled);
        assertEquals("ProduceResults", plan.root.operator);
        assertNull(plan.root.actual, "EXPLAIN carries no metrics");
        assertTrue(plan.statement.startsWith("MATCH (p:Person)"));

        // The cartesian MATCH's warning carries its remedy, joined per operator.
        PlanDocument warned =
                conn.query("EXPLAIN MATCH (a:Person), (b:City) RETURN a, b").getPlan();
        List<Notification> warnings = planWarnings(warned.root);
        assertEquals(1, warnings.size(), "one cartesian warning");
        assertEquals("Neo.ClientNotification.Statement.CartesianProduct", warnings.get(0).getCode());
        assertFalse(warnings.get(0).getRemedy().isEmpty(), "the warning carries its remedy");

        // A plain run carries null, never an empty document.
        assertNull(conn.query("RETURN 1 AS n").getPlan());
    }

    @Test
    void anOutOfRangeOperatorIdRefusesInsteadOfTruncating() throws Exception {
        // An integral operator_id outside the long range is a refusal, not
        // longValue()'s silent truncation to the low 64 bits
        com.fasterxml.jackson.databind.JsonNode node = Json.STRICT.readTree(
                "{\"operator_id\": 9223372036854775808}");
        RelateException e = assertThrows(RelateException.class,
                () -> Json.optionalLong(node, "operator_id", "notification entry",
                        node.toString()));
        assertTrue(e.getMessage().contains("operator_id"), "names the key: " + e.getMessage());
    }

    private static List<Notification> planWarnings(PlanNode node) {
        List<Notification> out = new ArrayList<>(node.warnings);
        for (PlanNode child : node.children) {
            out.addAll(planWarnings(child));
        }
        return out;
    }
}
