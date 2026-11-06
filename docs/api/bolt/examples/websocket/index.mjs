// Official Neo4j JavaScript driver over Bolt-over-WebSocket (ADR-0266).
//
// Imports the driver's prebuilt browser ESM bundle, which dials through the
// global WebSocket — the same Bolt-over-WebSocket channel neo4j-browser
// uses. The global WebSocket is wrapped below so the script records every
// dialed URL and fails unless a ws:// URL was actually opened, proving the
// WebSocket channel (not a raw TCP channel) carried the session.
//
// Run against a server started with `--auth none`; the server ignores the
// basic-auth credentials the driver sends in HELLO.
//
//   relate serve --host 127.0.0.1 --bolt 7687 --database :memory: --auth none
//   RELATE_BOLT_PORT=7687 node index.mjs

import neo4j from 'neo4j-driver/lib/browser/neo4j-web.esm.js'

const dialedUrls = []
const NativeWebSocket = globalThis.WebSocket
if (typeof NativeWebSocket !== 'function') {
  console.error('global WebSocket is not available (Node 22+ required)')
  process.exit(1)
}
globalThis.WebSocket = class RecordingWebSocket extends NativeWebSocket {
  constructor(url, protocols) {
    dialedUrls.push(String(url))
    super(url, protocols)
  }
}

const port = process.env.RELATE_BOLT_PORT || '7687'
const uri = `bolt://127.0.0.1:${port}`

function fail(message) {
  console.error(`FAIL: ${message}`)
  process.exit(1)
}

// disableLosslessIntegers so results come back as plain JS numbers.
const driver = neo4j.driver(uri, neo4j.auth.basic('neo4j', 'unused'), {
  disableLosslessIntegers: true,
})

try {
  // 1. Simple read: RETURN 1 AS n -> 1.
  const read = await driver.executeQuery('RETURN 1 AS n')
  if (read.records.length !== 1 || read.records[0].get('n') !== 1) {
    fail('RETURN 1 AS n did not return 1')
  }

  // 2. Parameterized write, then read it back.
  await driver.executeQuery('CREATE (p:Person {name: $name})', {
    name: 'Ada',
  })
  const match = await driver.executeQuery(
    'MATCH (p:Person {name: $name}) RETURN p.name AS name',
    { name: 'Ada' },
  )
  if (match.records.length !== 1 || match.records[0].get('name') !== 'Ada') {
    fail('MATCH did not return the created node')
  }

  // 3. Temporal and spatial results hydrate as the JS driver's native
  //    types (the LABS-323 Bolt compat fixes): a date as neo4j.types.Date,
  //    a datetime as neo4j.types.DateTime (nanoseconds and the +01:00
  //    offset kept), a duration as neo4j.types.Duration, and cartesian
  //    and geographic points as neo4j.types.Point (float coordinates,
  //    srid 7203 / 4326) — never strings.
  const typed = await driver.executeQuery(
    "RETURN date('2024-01-02') AS d, " +
      "datetime('2024-01-02T03:04:05.123456789+01:00') AS dt, " +
      "duration('P1Y2M3DT4H5M6.5S') AS dur, " +
      'point({srid: 7203, x: 1.5, y: 2.5}) AS c, ' +
      'point({longitude: 13, latitude: 55.6}) AS g',
  )
  const typedRow = typed.records[0]
  const d = typedRow.get('d')
  const dt = typedRow.get('dt')
  const dur = typedRow.get('dur')
  const c = typedRow.get('c')
  const g = typedRow.get('g')
  if (
    !(d instanceof neo4j.types.Date) ||
    d.year !== 2024 ||
    d.month !== 1 ||
    d.day !== 2
  ) {
    fail(`date did not hydrate as neo4j.types.Date(2024-01-02): ${String(d)}`)
  }
  if (
    !(dt instanceof neo4j.types.DateTime) ||
    String(dt) !== '2024-01-02T03:04:05.123456789+01:00' ||
    dt.timeZoneOffsetSeconds !== 3600
  ) {
    fail(
      `datetime did not hydrate as a +01:00 neo4j.types.DateTime: ${String(dt)}`,
    )
  }
  if (
    !(dur instanceof neo4j.types.Duration) ||
    dur.months !== 14 ||
    dur.days !== 3 ||
    dur.seconds !== 14706 ||
    dur.nanoseconds !== 500000000
  ) {
    fail(`duration did not hydrate as neo4j.types.Duration: ${String(dur)}`)
  }
  if (
    !(c instanceof neo4j.types.Point) ||
    c.srid !== 7203 ||
    c.x !== 1.5 ||
    c.y !== 2.5
  ) {
    fail(`2D point did not hydrate as a srid-7203 neo4j.types.Point: ${String(c)}`)
  }
  if (
    !(g instanceof neo4j.types.Point) ||
    g.srid !== 4326 ||
    g.x !== 13 ||
    g.y !== 55.6
  ) {
    fail(`geographic point did not hydrate as a srid-4326 neo4j.types.Point: ${String(g)}`)
  }

  // 4. A write summary reports what the write did: queryType 'w' and the
  //    counters nodesCreated/labelsAdded/relationshipsCreated.
  const written = await driver.executeQuery(
    'CREATE (a:WsProbe {v: 1})-[r:WS_PROBE]->(b:WsProbe2 {v: 2})',
  )
  const updates = written.summary.counters.updates()
  if (
    written.summary.queryType !== 'w' ||
    updates.nodesCreated !== 2 ||
    updates.labelsAdded !== 2 ||
    updates.relationshipsCreated !== 1 ||
    !written.summary.counters.containsUpdates()
  ) {
    fail(
      `write summary did not report type 'w' and nodesCreated: ` +
        `${written.summary.queryType} ${JSON.stringify(updates)}`,
    )
  }

  // 5. Explicit transaction: visible after commit.
  const session = driver.session()
  const tx = session.beginTransaction()
  try {
    await tx.run('CREATE (p:Person {name: $name})', { name: 'Lin' })
    await tx.commit()
  } catch (err) {
    await tx.rollback()
    throw err
  } finally {
    await session.close()
  }
  const committed = await driver.executeQuery(
    'MATCH (p:Person {name: $name}) RETURN count(p) AS c',
    { name: 'Lin' },
  )
  if (committed.records[0].get('c') !== 1) {
    fail('explicit transaction result not visible after commit')
  }
} catch (err) {
  fail(err && err.message ? err.message : String(err))
} finally {
  await driver.close()
}

// 6. Prove the WebSocket channel was used: the browser bundle must have
//    dialed a ws:// URL through the (wrapped) global WebSocket.
if (!dialedUrls.some((u) => u.startsWith('ws://'))) {
  fail(`no ws:// URL was dialed (saw: ${dialedUrls.join(', ') || 'none'})`)
}

console.log('ok')
