# Example: temporal, spatial and bytes values over Bolt

A RUN's second field is a PackStream map of parameters. Bolt 4.4 drivers
send temporal, duration and point values as structs; the engine decodes the
documented tags (ADR-0177) instead of silently turning them into `null`
(see [Parameter value
decoding](../spec.md#parameter-value-decoding-adr-0177)). Results go the
other way: temporal and point values come back as the same PackStream
structures (ADR-0325, ADR-0326) and byte arrays as PackStream Bytes
(ADR-0328), so the driver hydrates its native types. This example proves
both directions with the official Neo4j Python driver.

In one terminal, run from the repository root (`--auth none` makes the
server ignore the placeholder credentials the driver sends):

```bash
cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --auth none
```

In another terminal with the official Neo4j Python driver installed
(`python3 -m pip install neo4j`), run the [driver example](temporal_params.py)
from the repository root:

```bash
python3 docs/api/bolt/examples/temporal_params.py
```

The example sends one Python value per wire struct — `datetime.date`,
fixed-offset and naive `datetime.time`, fixed-offset, zoned
(`Europe/Stockholm`, a pytz zone: the driver's utc-patch encoder mishandles
`zoneinfo.ZoneInfo`) and naive `datetime.datetime`, `neo4j.time.Duration`,
and 2D/3D `CartesianPoint` plus `WGS84Point` — and asserts each equals the
value its Cypher constructor produces. The Python driver negotiates the
`patch_bolt` `utc` patch (ADR-0329), so its datetimes cross as the UTC
structures `0x49`/`0x69`. It then returns one value of every result struct
— a date, a time, a localtime, an offset and a zoned datetime, a
localdatetime, a duration, 2D and 3D cartesian points and a WGS-84 point —
and asserts they hydrate as `neo4j.time.Date`, `neo4j.time.Time` (naive for
localtime), `neo4j.time.DateTime` (nanoseconds and the `Europe/Stockholm`
zone kept, naive for localdatetime), `neo4j.time.Duration`,
`neo4j.spatial.CartesianPoint` and `neo4j.spatial.WGS84Point`. Last, it
sends a `bytearray` and asserts the same bytes come back. CI runs this same
script in the `relate-bolt` job.
