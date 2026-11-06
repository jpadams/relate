/*
 * Explicit multi-statement transaction (ADR-0028): begin, a write, a read
 * of the written data through the walker, then commit -- whose bookmark
 * out-param hands over the transaction's bookmark (caller-owned).
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include <string.h>
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
    if (relate_conn_begin(conn, &err) != RELATE_OK) {
        take_error("begin", err);
        return 1;
    }
    if (relate_query_params(conn, "CREATE (n:Person {name: 'Ada'})", NULL, &result, &err)
        != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    relate_destroy_result(result);
    result = NULL;
    if (relate_query_params(conn, "MATCH (n:Person) RETURN n.name AS name", NULL, &result, &err)
        != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    /* A string cell: the walker hands the payload and its byte length --
     * a Cypher string can carry an interior NUL, so read the pair. */
    size_t len = 0;
    const char *name = relate_value_string(relate_value_at(result, 0, 0), &len);
    if (name == NULL || len != 3 || memcmp(name, "Ada", 3) != 0) {
        fprintf(stderr, "expected the string 'Ada'\n");
        return 1;
    }
    /* Inside the explicit transaction the statement committed nothing:
     * its result bookmark is NULL -- the commit's bookmark is coming. */
    if (relate_result_bookmark(result) != NULL) {
        fprintf(stderr, "expected no result bookmark inside a transaction\n");
        return 1;
    }
    printf("%s\n", name);
    relate_destroy_result(result);
    result = NULL;

    err = NULL;
    char *bookmark = NULL;
    if (relate_conn_commit(conn, &bookmark, &err) != RELATE_OK) {
        take_error("commit", err);
        return 1;
    }
    printf("commit bookmark: %s\n", bookmark);
    relate_free_string(bookmark);

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
