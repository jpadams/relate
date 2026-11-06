package relate_test

import (
	"strings"
	"testing"

	"github.com/relatedb/relate/clients/go"
)

// openConn opens an in-memory database and one connection, closed at test
// end. The external test package's shared fixture lives here, in the
// lowest shared module, not inside either consumer's test file.
func openConn(t *testing.T) *relate.Connection {
	t.Helper()
	db, err := relate.Open(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(db.Close)
	conn, err := db.Connect()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(conn.Close)
	return conn
}

// requireErrorMentions pins a decoder's refusal: decoding raw fails with
// an error mentioning token. One copy of that assertion, shared by the
// error-shape and stats decode-contract tests' missing-key, mistyped,
// null-payload and closed-vocabulary checks.
func requireErrorMentions[T any](t *testing.T, decode func(string) (T, error), raw, token, what string) {
	t.Helper()
	if got, err := decode(raw); err == nil || !strings.Contains(err.Error(), token) {
		t.Fatalf("%s: got (%#v, %v), want an error mentioning %q", what, got, err, token)
	}
}

// requireMissingKey is requireErrorMentions for a required key: a payload
// whose key is missing — or null, which the shape treats as absent —
// fails with an error naming the key.
func requireMissingKey[T any](t *testing.T, decode func(string) (T, error), key, raw string) {
	t.Helper()
	requireErrorMentions(t, decode, raw, `missing "`+key+`"`, "missing "+key)
}
