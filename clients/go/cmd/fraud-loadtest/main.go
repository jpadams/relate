package main

import (
	"os"

	"github.com/relatedb/relate/clients/go/loadtest"
)

func main() {
	os.Exit(loadtest.RunCLI(loadtest.FraudCLI(loadtest.RunFraud, loadtest.PrintFraud),
		os.Args[1:], os.Stdout, os.Stderr))
}
