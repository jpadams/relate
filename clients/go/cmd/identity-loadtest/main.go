package main

import (
	"os"

	"github.com/relatedb/relate/clients/go/loadtest"
)

func main() {
	os.Exit(loadtest.RunCLI(loadtest.IdentityCLI(loadtest.RunIdentity, loadtest.PrintIdentity),
		os.Args[1:], os.Stdout, os.Stderr))
}
