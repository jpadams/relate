/*
 * The typed value walker (ADR-0502 §1): a complete recursive decoder over
 * every kind. Handles are owned by the result (no per-value free);
 * out-of-kind access is a defined refusal that never poisons the handle.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include "relate.h"

void print_value(relate_value v);

static void print_bytes(const char *s, size_t len) {
    putchar('"');
    for (size_t i = 0; i < len; i++) {
        if (s[i] == '\0') fputs("\\u0000", stdout); else putchar(s[i]);
    }
    putchar('"');
}

static int64_t comp(relate_status (*get)(relate_value, int64_t *), relate_value v) {
    int64_t out = 0;
    get(v, &out); /* in-kind by construction: the caller switched on the kind */
    return out;
}

static void print_wall_clock(relate_value v) {
    printf("%02" PRId64 ":%02" PRId64 ":%02" PRId64 ".%09" PRId64,
           comp(relate_value_hour, v), comp(relate_value_minute, v),
           comp(relate_value_second, v), comp(relate_value_nanosecond, v));
}

static void print_date(relate_value v) {
    printf("%04" PRId64 "-%02" PRId64 "-%02" PRId64, comp(relate_value_year, v),
           comp(relate_value_month, v), comp(relate_value_day, v));
}

static void print_offset_and_zone(relate_value v) {
    printf("%+" PRId64 "s", comp(relate_value_offset_seconds, v));
    const char *zone = relate_value_zone(v); /* "" when offset-only */
    if (zone != NULL && zone[0] != '\0') printf("[%s]", zone);
}

static void print_list(relate_value v) {
    long long n = relate_value_list_len(v);
    putchar('[');
    for (long long i = 0; i < n; i++) {
        if (i) fputs(", ", stdout);
        print_value(relate_value_list_get(v, i)); /* child: result-owned */
    }
    putchar(']');
}

static void print_map(relate_value v) {
    long long n = relate_value_map_len(v);
    putchar('{');
    for (long long i = 0; i < n; i++) {
        size_t len = 0;
        const char *key = relate_value_map_key(v, i, &len);
        if (i) fputs(", ", stdout);
        print_bytes(key, len);
        fputs(": ", stdout);
        print_value(relate_value_map_get(v, key));
    }
    putchar('}');
}

static int is_int_dtype(const char *dtype) {
    static const char *const ints[] = {"INTEGER8", "INTEGER16", "INTEGER32", "INTEGER"};
    for (size_t i = 0; i < sizeof ints / sizeof ints[0]; i++) {
        if (strcmp(dtype, ints[i]) == 0) return 1;
    }
    return 0;
}

void print_value(relate_value v) {
    int kind = relate_value_kind(v);
    switch (kind) {
    case -1:
        fputs("<no value>", stdout); /* a NULL handle, not Cypher null */
        return;
    case RELATE_V_NULL:
        fputs("null", stdout);
        return;
    case RELATE_V_BOOL: {
        uint8_t b = 0;
        relate_value_bool(v, &b);
        fputs(b ? "true" : "false", stdout);
        return;
    }
    case RELATE_V_INT:
        printf("%" PRId64, comp(relate_value_int, v));
        return;
    case RELATE_V_FLOAT: {
        double d = 0.0;
        relate_value_float(v, &d); /* NaN and +-Inf arrive as themselves */
        printf("%.17g", d);
        return;
    }
    case RELATE_V_STRING: {
        size_t len = 0;
        const char *s = relate_value_string(v, &len);
        print_bytes(s, len);
        return;
    }
    case RELATE_V_NODE:
        printf("(%s #%" PRId64 " ", relate_value_element_id(v),
               comp(relate_value_id, v)); /* the deprecated numeric id */
        print_list(v); /* labels */
        putchar(' ');
        print_map(v); /* properties */
        putchar(')');
        return;
    case RELATE_V_RELATIONSHIP:
        printf("[%s #%" PRId64 ":%s %s #%" PRId64 "->%s #%" PRId64 " ",
               relate_value_element_id(v), comp(relate_value_id, v),
               relate_value_string(v, NULL), /* the type */
               relate_value_start_element_id(v), comp(relate_value_start_id, v),
               relate_value_end_element_id(v), comp(relate_value_end_id, v));
        print_map(v);
        putchar(']');
        return;
    case RELATE_V_PATH: /* node, rel, node, ... */
    case RELATE_V_LIST:
        print_list(v);
        return;
    case RELATE_V_MAP:
        print_map(v);
        return;
    case RELATE_V_POINT: {
        int32_t srid = 0, dim = 0;
        double x = 0, y = 0, z = 0;
        relate_value_point(v, &srid, &dim, &x, &y, &z);
        printf("point(srid=%d, %g, %g", srid, x, y);
        if (dim == 3) printf(", %g", z);
        putchar(')');
        return;
    }
    case RELATE_V_DATE:
        print_date(v);
        return;
    case RELATE_V_TIME:
        print_wall_clock(v);
        print_offset_and_zone(v);
        return;
    case RELATE_V_LOCAL_TIME:
        print_wall_clock(v);
        return;
    case RELATE_V_DATETIME:
        print_date(v);
        putchar('T');
        print_wall_clock(v);
        print_offset_and_zone(v);
        return;
    case RELATE_V_LOCAL_DATETIME:
        print_date(v);
        putchar('T');
        print_wall_clock(v);
        return;
    case RELATE_V_DURATION: {
        int64_t months = 0, days = 0, seconds = 0, nanos = 0;
        relate_value_duration(v, &months, &days, &seconds, &nanos);
        printf("duration(months=%" PRId64 ", days=%" PRId64 ", seconds=%" PRId64
               ", nanos=%" PRId64 ")", months, days, seconds, nanos);
        return;
    }
    case RELATE_V_VECTOR: {
        const char *dtype = relate_value_vector_dtype(v);
        long long dim = relate_value_vector_dim(v);
        printf("vector<%s>[", dtype);
        for (long long i = 0; i < dim; i++) {
            if (i) fputs(", ", stdout);
            if (is_int_dtype(dtype)) {
                int64_t n = 0;
                relate_value_vector_int(v, i, &n);
                printf("%" PRId64, n);
            } else {
                double d = 0.0;
                relate_value_vector_float(v, i, &d);
                printf("%g", d);
            }
        }
        putchar(']');
        return;
    }
    default:
        printf("<unsupported value kind %d>", kind); /* a later, additive ABI */
        return;
    }
}

static void take_error(const char *what, char *err) {
    fprintf(stderr, "%s failed: %s\n", what, err ? err : "(no detail)");
    relate_free_string(err);
}

int main(void) {
    relate_database db = NULL;
    relate_connection conn = NULL;
    relate_query_result r = NULL;
    char *err = NULL;

    if (relate_database_open(":memory:", &db, &err) != RELATE_OK) {
        take_error("database_open", err);
        return 1;
    }
    if (relate_connect_namespace(db, NULL, &conn, &err) != RELATE_OK) {
        take_error("connect_namespace", err);
        return 1;
    }
    /* One row over most of the kinds: scalars with an interior NUL and a
     * beyond-2^53 int, an entity pair, containers, temporals, a point and
     * a vector. */
    if (relate_query_params(
             conn,
             "CREATE (a:Person {name: 'Ada'})-[k:KNOWS {since: 1833}]->"
             "(b:Person {name: 'Charles'}) "
             "RETURN 1 AS one, 9007199254740993 AS big, 'a\\u0000b' AS nul, a, k, "
             "[x IN [1.5, null] | x] AS xs, "
             "{m: duration('P1Y2M'), t: time('09:30+01:00')} AS m, "
             "datetime({year: 2026, month: 11, day: 4, timezone: 'Europe/Berlin'}) AS dt, "
             "point({x: 1, y: 2, z: 3}) AS p, vector([1, 2, 3], 3, INTEGER8) AS v, "
             "null AS nothing",
             NULL, &r, &err) != RELATE_OK) {
        take_error("query_params", err);
        return 1;
    }
    if (relate_result_is_error(r)) {
        fprintf(stderr, "statement failed: %s\n", relate_result_error_json(r));
        return 1;
    }
    for (long long col = 0; col < relate_column_count(r); col++) {
        printf("%s = ", relate_column_name(r, col));
        print_value(relate_value_at(r, 0, col));
        putchar('\n');
    }

    /* Cypher null is a handle of kind NULL -- never a NULL handle. */
    if (relate_value_kind(relate_value_at(r, 0, 10)) != RELATE_V_NULL) {
        fprintf(stderr, "expected kind NULL for the null cell\n");
        return 1;
    }
    /* Out-of-kind access is a defined refusal: the status is the report,
     * the out-param is untouched, and the handle stays usable. */
    int64_t n = 42;
    if (relate_value_int(relate_value_at(r, 0, 2), &n) != RELATE_ERROR || n != 42) {
        fprintf(stderr, "expected value_int to refuse a STRING\n");
        return 1;
    }
    size_t len = 0;
    const char *s = relate_value_string(relate_value_at(r, 0, 2), &len);
    if (s == NULL || len != 3 || memcmp(s, "a\0b", 3) != 0) {
        fprintf(stderr, "expected the interior-NUL string to cross whole\n");
        return 1;
    }
    /* Numeric entity ids (ADR-0506): the INTEGER id() returns, and the
     * same refusal for a non-entity. */
    int64_t eid = -1;
    if (relate_value_id(relate_value_at(r, 0, 3), &eid) != RELATE_OK || eid != 0) {
        fprintf(stderr, "expected value_id to read the first node's id\n");
        return 1;
    }
    eid = 42;
    if (relate_value_id(relate_value_at(r, 0, 2), &eid) != RELATE_ERROR || eid != 42) {
        fprintf(stderr, "expected value_id to refuse an INT\n");
        return 1;
    }
    relate_destroy_result(r);

    relate_disconnect(conn);
    relate_database_close(db);
    return 0;
}
