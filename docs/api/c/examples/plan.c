/*
 * The agent-native plan document (LABS-370, ADR-0381): read a statement's
 * structured plan as a `relate.plan/v1` JSON document. The pointer is
 * owned by the result and valid until relate_destroy_result() -- do
 * not free it. A statement that carried no plan returns NULL.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include "relate.h"

static void take_error(const char *what, char *err) {
    fprintf(stderr, "%s failed: %s\n", what, err ? err : "(no detail)");
    relate_free_string(err);
}

int main(void) {
    relate_database db = NULL;
    relate_connection conn = NULL;
    relate_query_result result = NULL;
    char *err = NULL;

    if (relate_database_open(":memory:", &db, &err) != RELATE_OK) {
        take_error("database_open", err);
        return 1;
    }
    if (relate_connect_namespace(db, NULL, &conn, &err) != RELATE_OK) {
        take_error("connect_namespace", err);
        return 1;
    }
    if (relate_query_params(conn, "CREATE (n:Person {name: 'Alice'})", NULL, &result, &err)
        != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    relate_destroy_result(result);
    result = NULL;

    /* EXPLAIN: the plan document, `profiled` false and no runtime metrics.
     * The document is snake_case with real numbers -- no nested args map and
     * no rendered text to parse, unlike the Neo4j-facing shape the Bolt and
     * HTTP surfaces emit for Neo4j's own tooling. */
    if (relate_query_params(conn,
                                "EXPLAIN MATCH (n:Person) RETURN n.name AS name",
                                NULL, &result, &err) != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    printf("explain=%s\n", relate_result_plan_json(result));
    relate_destroy_result(result);
    result = NULL;

    /* PROFILE: `profiled` true, every node carrying its `actual` metrics --
     * rows, time_ms, total_time_ms (own time plus every descendant's),
     * db_hits, memory_bytes and the read-kind mix. */
    if (relate_query_params(conn,
                                "PROFILE MATCH (n:Person) RETURN n.name AS name",
                                NULL, &result, &err) != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    printf("profile=%s\n", relate_result_plan_json(result));
    relate_destroy_result(result);
    result = NULL;

    /* A plain run plans nothing: the accessor returns NULL, never a
     * document (the C convention for absent, like the out-of-range result
     * accessors). */
    if (relate_query_params(conn, "RETURN 1 AS n", NULL, &result, &err) != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    printf("plain_is_null=%s\n",
           relate_result_plan_json(result) == NULL ? "yes" : "no");
    relate_destroy_result(result);

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
