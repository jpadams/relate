# Import and Export Data

## Import CSV with LOAD CSV

The `LOAD CSV` clause reads a CSV file and makes each row available as a variable in your Cypher query.

### With headers

If your CSV has a header row:

```csv
name,age,city
Alice,30,NYC
Bob,28,LA
Charlie,35,Chicago
```

```cypher
LOAD CSV WITH HEADERS FROM '/path/to/people.csv' AS row
CREATE (:Person {name: row.name, age: row.age, city: row.city});
```

Each `row` is a map where keys are the column headers.

### Without headers

If your CSV has no header row:

```csv
Alice,30,NYC
Bob,28,LA
```

```cypher
LOAD CSV FROM '/path/to/people.csv' AS row
CREATE (:Person {name: row[0], age: row[1], city: row[2]});
```

Each `row` is a list of string values accessed by index.

### Custom field separator

Use `FIELDTERMINATOR` for files that use a separator other than commas:

```cypher
LOAD CSV WITH HEADERS FROM '/path/to/data.tsv' AS row FIELDTERMINATOR '\t'
CREATE (:Record {id: row.id, value: row.value});
```

The terminator must be exactly one character — a multi-character (or empty)
`FIELDTERMINATOR` string fails with a type error instead of being truncated
to its first character.

## Import CSV with dot-commands (withdrawn)

The `.import` dot-command (csv / json / graphml / cypher) was withdrawn with
the interactive shell surface reduction (ADR-0058) — entering it errors
`Error: unknown command .import`. Use `LOAD CSV` (above) for CSV files, or
pipe a Cypher script into the shell:

```bash
./relate ./mydata < setup.cypher
```

## Export query results

Use the output-format flags with shell redirection. CSV:

```bash
./relate -csv ./mydata "MATCH (p:Person) RETURN p.name, p.age" > people.csv
```

```
p.name,p.age
Alice,30
Bob,28
```

JSON (`--json`, or one object per line with `--mode jsonl`):

```bash
./relate --json ./mydata "MATCH (p:Person) RETURN p.name, p.age" > people.json
```

The `.export` dot-command (csv / json / graphml) and the `.mode` / `.output`
REPL commands were withdrawn with the shell surface reduction (ADR-0058).
To export a whole database directory, copy the directory after a
`.checkpoint` — see [Backup and Restore](./backup-restore.md).

## Tips

- All values from CSV files are strings. Use `toInteger()` or `toFloat()` if you need numeric types:

  ```cypher
  LOAD CSV WITH HEADERS FROM 'data.csv' AS row
  CREATE (:Person {name: row.name, age: toInteger(row.age)});
  ```

- For large imports, consider using `FOREACH` with `UNWIND` for batch operations.

- File paths in `LOAD CSV` are relative to where you launched Relate, or can be absolute paths.

- Use **CSV** for tabular node data or interoperability with spreadsheets, and the `--json` / `--mode jsonl` output flags for programmatic interchange.
