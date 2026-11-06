package main

import (
	"fmt"
	"os"

	"github.com/relatedb/relate/clients/go/loadtest"
)

func main() {
	cfg := loadtest.DefaultIdentity()
	cfg.Ops = 8
	cfg.Warmup = 2
	cfg.Out = ""
	r, err := loadtest.RunIdentity(cfg)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(r.Name, r.Throughput.OpsPerSec)
}
