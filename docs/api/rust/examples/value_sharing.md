# Value-sharing example

Run with `cargo run -p relate --example value_sharing`.

`Value::String`, `Value::List`, `Value::Map` and `Value::Vector` hold their
payloads behind an `Arc` (`Arc<str>`, `Arc<Vec<Value>>`,
`Arc<BTreeMap<String, Value>>`, and `Arc<Vec<Value>>` in the `values` field;
ADR-0154, ADR-0170), so cloning a `Value` shares the payload in O(1) and
values are immutable. The example runs a query that returns a list, a map, a
string and a vector, clones each result `Value` and checks with `Arc::ptr_eq`
that the clone points at the same payload. It reads the payloads through
deref (`items.first()`, `fields.get("name")`, `&**s`), takes an owned copy of
the map with `Arc::unwrap_or_clone` (one copy, because the result row still
shares it) and checks the row is unchanged. It then passes a List parameter
built with `.into()` and a Map parameter built with `Arc::new(..)` through
`query_with_params` and checks both come back equal.
