/*
 * The typed params builder (ADR-0502 §4): scalars, nested list/map
 * children, temporals (a zoned datetime keeps its zone), a point, a
 * vector; then reuse of one root for a second statement; then a defined
 * refusal (a duplicate map name), reported through the error_json
 * out-param and leaving the builder usable.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include <string.h>
#include "relate.h"

/*
 * Report a putter refusal: one helper, so every put stays one line. It takes
 * the out-param's ADDRESS and reads *err only after the putter has returned:
 * passing `err` by value beside `&err` in one argument list would let the
 * compiler read it before the putter writes it (C leaves argument
 * evaluation order unspecified), printing no detail and leaking the string.
 */
static int put_ok(relate_status rc, char **err) {
    if (rc == RELATE_OK) return 1;
    fprintf(stderr, "put refused: %s\n", *err ? *err : "(no detail)");
    relate_free_string(*err);
    *err = NULL;
    return 0;
}

static void take_error(const char *what, char *err) {
    fprintf(stderr, "%s failed: %s\n", what, err ? err : "(no detail)");
    relate_free_string(err);
}

/* One canonical statement-failure report: 1 when the result is an error. */
static int result_failed(relate_query_result r) {
    if (!relate_result_is_error(r)) return 0;
    fprintf(stderr, "statement failed: %s\n", relate_result_error_json(r));
    return 1;
}

static int bind_and_run(relate_connection conn) {
    relate_params p = relate_params_new();
    relate_params tags = NULL, opts = NULL;
    char *err = NULL;
    const char *name = "Ada";
    double z = 30.5;
    double embedding[3] = {0.25, -1.0, 0.5};
    int ok =
        put_ok(relate_params_put_string(p, "name", name, strlen(name), &err), &err) &&
        put_ok(relate_params_put_int(p, "big", 9007199254740993LL, &err), &err) &&
        /* a list child: NULL names, appended in order */
        put_ok(relate_params_put_list(p, "tags", &tags, &err), &err) &&
        put_ok(relate_params_put_string(tags, NULL, "x", 1, &err), &err) &&
        put_ok(relate_params_put_int(tags, NULL, 2, &err), &err) &&
        /* a map child: named entries */
        put_ok(relate_params_put_map(p, "opts", &opts, &err), &err) &&
        put_ok(relate_params_put_bool(opts, "strict", 1, &err), &err) &&
        /* a zoned datetime keeps its zone; offset_seconds is ignored */
        put_ok(relate_params_put_datetime(p, "at", 2026, 11, 4, 9, 30, 0, 0, 0,
                                              "Europe/Berlin", &err), &err) &&
        /* a TIME takes an offset (here +01:00) and keeps no zone id */
        put_ok(relate_params_put_time(p, "t", 9, 30, 0, 0, 3600, NULL, &err), &err) &&
        put_ok(relate_params_put_duration(p, "d", 14, 0, 36 * 3600, 0, &err), &err) &&
        put_ok(relate_params_put_point(p, "loc", 4979, 12.5, 55.6, &z, &err), &err) &&
        put_ok(relate_params_put_vector_float(p, "emb", "FLOAT32", embedding, 3, &err), &err);
    if (!ok) {
        relate_params_free(p);
        return -1;
    }

    relate_query_result r = NULL;
    if (relate_query_params(conn, "RETURN $name, $big, $tags, $opts.strict, $at, $emb",
                                p, &r, &err) != RELATE_OK) {
        take_error("query_params", err);
        relate_params_free(p);
        return -1;
    }
    int failed = result_failed(r);
    relate_destroy_result(r);

    /* Reuse: the builder was read, not consumed. Add a name and run again.
     * The put and the second run are checked one at a time: a refused put
     * and a failed run are distinct failures -- neither folds into the
     * other's boolean, and a query_params ERROR is reported, not skipped. */
    if (!failed) {
        if (!put_ok(relate_params_put_null(p, "missing", &err), &err)) {
            relate_params_free(p);
            return -1;
        }
        if (relate_query_params(conn, "RETURN $missing IS NULL", p, &r, &err)
            != RELATE_OK) {
            take_error("query_params", err);
            relate_params_free(p);
            return -1;
        }
        failed = result_failed(r);
        relate_destroy_result(r);
    }
    relate_params_free(p); /* frees tags and opts too */
    return failed ? -1 : 0;
}

int main(void) {
    relate_database db = NULL;
    relate_connection conn = NULL;
    char *err = NULL;

    if (relate_database_open(":memory:", &db, &err) != RELATE_OK) {
        take_error("database_open", err);
        return 1;
    }
    if (relate_connect_namespace(db, NULL, &conn, &err) != RELATE_OK) {
        take_error("connect_namespace", err);
        return 1;
    }
    if (bind_and_run(conn) != 0) return 1;

    /* A defined refusal: a duplicate map name. The builder is unchanged
     * and still usable, and the error shape rides the out-param. */
    relate_params p = relate_params_new();
    int ok = put_ok(relate_params_put_int(p, "x", 1, &err), &err);
    if (ok) {
        ok = !put_ok(relate_params_put_int(p, "x", 2, &err), &err); /* refused */
    }
    /* The builder still works for a fresh name after the refusal. */
    if (ok) {
        ok = put_ok(relate_params_put_int(p, "y", 3, &err), &err);
    }
    relate_params_free(p);
    if (!ok) {
        fprintf(stderr, "expected the duplicate name to be refused and the builder to stay usable\n");
        return 1;
    }

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
