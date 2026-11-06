package main

import (
	"errors"
	"fmt"
	"os"

	"github.com/relatedb/relate/clients/go"
)

func main() {
	db, err := relate.Open(":memory:")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	defer db.Close()
	conn, err := db.Connect()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	defer conn.Close()

	// A failing statement returns a typed *relate.Error from the result's
	// error channel. Branch on its fields, never on the message text.
	_, err = conn.Query("RETURN 1/0 AS x")
	var e *relate.Error
	if errors.As(err, &e) {
		fmt.Println("code:", e.Code)
		fmt.Println("gql_status:", e.GQLStatus)
		fmt.Println("stage:", e.Stage)
		fmt.Println("retryable:", e.Retryable)
	}

	// A call that fails reports the same typed error through its own out-param.
	_, err = db.ConnectNamespace("nope")
	if errors.As(err, &e) {
		fmt.Println("connect message:", e.Message)
	}
}
