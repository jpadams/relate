# API documentation contract

Every public Relate surface is documented here.

| Surface | Path | Ships in |
| --- | --- | --- |
| Rust crate | [rust/](rust/) | `relate` (`relate::Database`) |
| C ABI | [c/](c/) | `relate-ffi` / `librelate_ffi` (`relate_*`) |
| CLI | [cli/](cli/) | `relate-cli` (binary `relate`) |
| Python | [python/](python/) | `relate-python` (PyO3 package `relate`) |
| JavaScript | [javascript/](javascript/) | `clients/javascript` |
| Java | [java/](java/) | `clients/java` |
| Go | [go/](go/) | `clients/go` |
| Swift (iOS / macOS) | [swift/](swift/) | `clients/swift` (SwiftPM `Relate`, root `Package.swift`) |
| WASM (browser) | [wasm/](wasm/) | `relate-wasm` (npm `@relate/relate-wasm`) |
| Bolt | [bolt/](bolt/) | `relate serve --bolt` |
| HTTP Query API v2 | [http/](http/) | `relate-http` |
| MCP | [mcp/](mcp/) | `relate serve --mcp` |
| CLI extensions | [extensions/](extensions/) | `relate-cli` (binary `relate`, the `run` mount) |
