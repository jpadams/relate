/*
 * Bookmarks (ADR-0502 §7): the engine's commit sequence, "relate:<n>".
 * The auto-commit write's bookmark rides its result (result-owned); the
 * explicit transaction's bookmark arrives through relate_conn_commit's
 * bookmark out-param (caller-owned, freed with relate_free_string). A
 * commit that crossed no WAL boundary reports RELATE_OK with a NULL
 * bookmark -- no bookmark is fabricated.
 *
 *   cc -I surfaces/relate-ffi/include example.c -L target/debug -lrelate_ffi
 */
#include <stdio.h>
#include "relate.h"

static void take_error(const char *what, char *err) {
    fprintf(stderr, "%s failed: %s\n", what, err ? err : "(no detail)");
    relate_free_string(err);
}

/* Run one statement, refusing both failure modes -- the one ladder every
 * query in this example shares: a RELATE_ERROR status is reported
 * through the out-param, a failing statement rides the result (its shape
 * printed). Returns the good result for the caller to read and destroy,
 * NULL on either failure. */
static relate_query_result run_statement(relate_connection conn,
                                             const char *cypher) {
    relate_query_result r = NULL;
    char *err = NULL;
    if (relate_query_params(conn, cypher, NULL, &r, &err) != RELATE_OK) {
        take_error("query_params", err);
        return NULL;
    }
    if (relate_result_is_error(r)) {
        fprintf(stderr, "statement failed: %s\n", relate_result_error_json(r));
        relate_destroy_result(r);
        return NULL;
    }
    return r;
}

static int write_and_commit(relate_connection conn) {
    char *err = NULL;

    /* Auto-commit write: the bookmark rides the result (checked before
     * the bookmark, never the other way round). */
    relate_query_result r = run_statement(conn, "CREATE (:Note {n: 1})"); /* NULL = failure */
    if (r == NULL) {
        return -1;
    }
    const char *bm = relate_result_bookmark(r); /* result-owned */
    printf("auto-commit bookmark: %s\n", bm ? bm : "(none)");
    if (bm == NULL) {
        fprintf(stderr, "expected a bookmark for the auto-commit write\n");
        relate_destroy_result(r);
        return -1;
    }
    relate_destroy_result(r);

    /* Explicit transaction: the bookmark arrives through commit's
     * bookmark out-param. */
    if (relate_conn_begin(conn, &err) != RELATE_OK) {
        take_error("begin", err);
        return -1;
    }
    /* inside a transaction the result bookmark is NULL either way; the
     * result is still caller-owned -- destroy it on every path */
    relate_query_result w = run_statement(conn, "CREATE (:Note {n: 2})");
    if (w == NULL) {
        relate_conn_rollback(conn, NULL);
        return -1;
    }
    relate_destroy_result(w);

    err = NULL;
    char *bookmark = NULL;
    if (relate_conn_commit(conn, &bookmark, &err) != RELATE_OK) {
        take_error("commit", err); /* check "retryable" before retrying */
        return -1;
    }
    if (bookmark != NULL) {
        printf("commit bookmark: %s\n", bookmark);
        relate_free_string(bookmark); /* caller-owned */
    } else {
        printf("committed nothing: no bookmark\n");
    }
    return 0;
}

/* A read-only transaction commits nothing: OK status, NULL bookmark --
 * the mirror of write_and_commit's explicit-transaction half. */
static int read_only_commit(relate_connection conn) {
    char *err = NULL;
    if (relate_conn_begin(conn, &err) != RELATE_OK) {
        take_error("begin", err);
        return -1;
    }

    relate_query_result n = run_statement(conn, "MATCH (n) RETURN count(n) AS n");
    if (n == NULL) {
        relate_conn_rollback(conn, NULL);
        return -1;
    }
    relate_destroy_result(n); /* the read's result is caller-owned too */

    /* A successful commit that crossed no WAL boundary reports OK with a
     * NULL bookmark -- no bookmark is fabricated. Two distinct checks:
     * a failed commit and an unexpected bookmark are different
     * surprises, reported as themselves. */
    err = NULL;
    char *bookmark = NULL;
    if (relate_conn_commit(conn, &bookmark, &err) != RELATE_OK) {
        take_error("commit", err);
        return -1;
    }
    if (bookmark != NULL) {
        fprintf(stderr, "expected no bookmark for a read-only commit, got %s\n", bookmark);
        relate_free_string(bookmark);
        return -1;
    }
    printf("read-only commit: no bookmark, as expected\n");
    return 0;
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

    int failed = write_and_commit(conn) != 0;
    if (!failed) {
        failed = read_only_commit(conn) != 0;
    }

    relate_disconnect(conn);
    relate_database_close(db);
    return failed ? 1 : 0; /* the examples' exit vocabulary: 0 or 1 */
}
