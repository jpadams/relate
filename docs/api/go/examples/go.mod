module github.com/relatedb/relate/docs/api/go/examples

go 1.22

require github.com/relatedb/relate/clients/go v0.0.0

// The examples live beside the client they document; the replace keeps
// them runnable from any checkout without a published module version
// (the same standalone-project recipe clients/go/README.md documents).
replace github.com/relatedb/relate/clients/go => ../../../../clients/go
