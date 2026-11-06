package main

import (
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
	if _, err := conn.Query("CREATE (n:Person {name: 'Ada'})"); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	res, err := conn.Query("MATCH (n:Person) RETURN n.name AS name")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(res.Rows[0][0])
}
