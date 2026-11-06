/*
 * The smallest v2 program: open, connect, run one statement, read one INT
 * cell through the typed value walker.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include "relate.h"

/* A caller-owned out-param error: print it and free it. */
static void take_error(const char *what, char *err) {
    fprintf(stderr, "%s failed: %s\n", what, err ? err : "(no detail)");
    relate_free_string(err);
}

int main(void) {
    if (relate_abi_version() != 2) {
        fprintf(stderr, "unsupported ABI %d\n", relate_abi_version());
        return 1;
    }
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
    if (relate_query_params(conn, "RETURN 1 AS n", NULL, &result, &err) != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    /* A statement failure would ride the result: always test before reading. */
    if (relate_result_is_error(result)) {
        fprintf(stderr, "statement failed: %s\n", relate_result_error_json(result));
        return 1;
    }
    /* The cell is a typed value: take the root handle, switch on the kind. */
    relate_value v = relate_value_at(result, 0, 0);
    int64_t n = 0;
    if (relate_value_kind(v) != RELATE_V_INT ||
        relate_value_int(v, &n) != RELATE_OK || n != 1) {
        fprintf(stderr, "expected INT 1\n");
        return 1;
    }
    printf("%s\n%lld\n", relate_column_name(result, 0), (long long)n);
    relate_destroy_result(result);
    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
