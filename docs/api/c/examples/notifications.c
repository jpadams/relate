/*
 * Query notifications (ADR-0363): read a result's notification list as the
 * engine-native JSON array. An EXPLAIN whose MATCH patterns share no node
 * variable plans as a Cartesian product, so its list carries the
 * Neo.ClientNotification.Statement.CartesianProduct entry; a plain run
 * derives no notifications and returns [].
 *
 * The pointer is owned by the result and valid until
 * relate_destroy_result() -- do not free it.
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

/* Run one statement and return its result, or NULL after printing the
 * out-param error. A statement failure still returns its (error) result:
 * the caller tests relate_result_is_error. */
static relate_query_result run(relate_connection conn, const char *query) {
    relate_query_result result = NULL;
    char *err = NULL;
    if (relate_query_params(conn, query, NULL, &result, &err) != RELATE_OK) {
        take_error("query_params", err);
        return NULL;
    }
    return result;
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

    /* One :Person and one :City anchor the two disconnected patterns. */
    result = run(conn, "CREATE (:Person)");
    if (result == NULL || relate_result_is_error(result)) return 1;
    relate_destroy_result(result);
    result = run(conn, "CREATE (:City)");
    if (result == NULL || relate_result_is_error(result)) return 1;
    relate_destroy_result(result);
    result = NULL;

    /* EXPLAIN derives the plan-time list: the CartesianProduct entry carries
     * code, gql_status, severity, category, title, description, remedy and
     * the raising operator's id. */
    result = run(conn, "EXPLAIN MATCH (a:Person), (b:City) RETURN a, b");
    if (result == NULL) return 1;
    const char *notifications = relate_result_notifications(result);
    if (notifications == NULL) {
        fprintf(stderr, "null notifications pointer\n");
        return 1;
    }
    if (strstr(notifications, "Neo.ClientNotification.Statement.CartesianProduct") == NULL) {
        fprintf(stderr, "expected a CartesianProduct notification, got %s\n", notifications);
        return 1;
    }
    printf("explain_notifications=%s\n", notifications);
    relate_destroy_result(result);

    /* A plain run does no notification work: the array is present and empty,
     * never NULL and never a re-derivation of the EXPLAIN list. */
    result = run(conn, "MATCH (a:Person), (b:City) RETURN a, b");
    if (result == NULL) return 1;
    notifications = relate_result_notifications(result);
    if (notifications == NULL || strcmp(notifications, "[]") != 0) {
        fprintf(stderr, "expected [], got %s\n",
                notifications ? notifications : "(null)");
        return 1;
    }
    printf("plain_notifications=%s\n", notifications);
    relate_destroy_result(result);

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
