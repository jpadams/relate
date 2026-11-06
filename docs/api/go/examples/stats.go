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

	// A write reports what it mutated (ADR-0216, the counting per
	// ADR-0309): two nodes, one relationship, two property writes, one
	// label per node.
	res, err := conn.Query("CREATE (p:Person {name: 'Ada'})-[:KNOWS]->(q:Person {name: 'Grace'})")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println("CREATE stats:", res.Stats.NodesCreated, "nodes,",
		res.Stats.RelationshipsCreated, "relationships,",
		res.Stats.PropertiesSet, "properties set,",
		res.Stats.LabelsAdded, "labels added")

	// A read reports all zeros — the canonical 11 are always present, so
	// there is no missing-field check before reading a counter.
	res, err = conn.Query("MATCH (p:Person) RETURN p.name AS name")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println("MATCH stats:  nodes created:", res.Stats.NodesCreated,
		"nodes deleted:", res.Stats.NodesDeleted)

	// Schema DDL counts in the constraint and index counters.
	res, err = conn.Query("CREATE CONSTRAINT person_name FOR (p:Person) REQUIRE p.name IS UNIQUE")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println("CREATE CONSTRAINT stats: constraints added:", res.Stats.ConstraintsAdded)

	// EXPLAIN does not execute: the wrapped write's counters stay zero.
	res, err = conn.Query("EXPLAIN CREATE (n)")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println("EXPLAIN stats: nodes created:", res.Stats.NodesCreated)
}
