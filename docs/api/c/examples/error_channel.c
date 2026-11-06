/*
 * The v2 error channel (ADR-0502 §2): one shape, two delivery points.
 * A statement failure rides its result (result-owned error JSON); every
 * other failure fills the call's own trailing char **error_json out-param
 * (caller-owned, freed with relate_free_string). There is no
 * thread-local error slot.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include "relate.h"

/* Report and free a caller-owned out-param error. Returns 1 if there was one. */
static int take_error(const char *what, char *err) {
    if (err == NULL) {
        return 0;
    }
    fprintf(stderr, "%s failed: %s\n", what, err);
    relate_free_string(err);
    return 1;
}

static int run_statement(relate_connection conn, const char *cypher) {
    relate_query_result r = NULL;
    char *err = NULL;
    if (relate_query_params(conn, cypher, NULL, &r, &err) != RELATE_OK) {
        take_error("query_params", err); /* connection-level: r is NULL */
        return -1;
    }
    if (relate_result_is_error(r)) {
        /* statement-level: result-owned, NOT freed by the caller */
        fprintf(stderr, "statement failed: %s\n", relate_result_error_json(r));
        relate_destroy_result(r);
        return -1;
    }
    printf("%lld row(s)\n", relate_row_count(r));
    relate_destroy_result(r);
    return 0;
}

static int open_and_connect(const char *uri, relate_database *db, relate_connection *conn) {
    char *err = NULL;
    if (relate_database_open(uri, db, &err) != RELATE_OK) {
        take_error("database_open", err); /* *db is NULL */
        return -1;
    }
    if (relate_connect_namespace(*db, NULL, conn, &err) != RELATE_OK) {
        take_error("connect_namespace", err);
        relate_database_close(*db);
        return -1;
    }
    return 0;
}

int main(void) {
    relate_database db = NULL;
    relate_connection conn = NULL;

    if (open_and_connect(":memory:", &db, &conn) != 0) return 1;

    /* A good statement: RELATE_OK, no error result. */
    if (run_statement(conn, "RETURN 1 AS n") != 0) return 1;

    /* A failing statement: STILL RELATE_OK -- the failure rides the
     * result. Checking only the status would treat this as success. */
    if (run_statement(conn, "RETURN 1/0") == 0) {
        fprintf(stderr, "expected RETURN 1/0 to be an error result\n");
        return 1;
    }

    /* A failing call: RELATE_ERROR, *out NULL, the shape in the
     * out-param. An unknown namespace fails "Graph not found: <name>". */
    relate_connection bad = NULL;
    char *err = NULL;
    if (relate_connect_namespace(db, "nope", &bad, &err) != RELATE_ERROR ||
        bad != NULL || err == NULL) {
        fprintf(stderr, "expected the unknown namespace to be refused\n");
        return 1;
    }
    printf("connect refused: %s\n", err);
    relate_free_string(err);

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
