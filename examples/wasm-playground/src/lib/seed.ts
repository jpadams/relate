import { templates } from './query-templates';

// Sample dataset for the playground, served two ways:
//
//   * SEED_DISPLAY_SCRIPT — a pretty LOAD CSV teaser shown in the welcome
//     dialog's editor and echoed into the terminal. Real LOAD CSV syntax, but
//     the browser build has no filesystem, so the engine can't actually read
//     these files.
//   * SEED_SCRIPT + seedParams() — what actually runs. UNWINDs $nodes and
//     $rels (parsed from the two embedded CSVs below) through dynamic labels
//     and types, producing the same graph the teaser describes.
//
// Keys: a Person is keyed by `name`, a Post by `id`, everything else by its
// one `name`. The CSV `key` column holds whichever is used by this row's
// relationships. INT_COLUMNS lists the columns that must round-trip through
// Number so the resulting properties match what `toIntegerOrNull(...)` in the
// LOAD CSV teaser would produce.

// LDBC-inspired mini-schema: Person / City / University / Company / Post / Tag.
// Kept small so every sample query returns a distinct, easily-inspectable
// result and the seed runs instantly.

const NODES_CSV = `label,key,name,country,id,age,content,createdAt
City,Amsterdam,Amsterdam,NL,,,,
City,Berlin,Berlin,DE,,,,
City,London,London,UK,,,,
City,Linköping,Linköping,SE,,,,
City,Göteborg,Göteborg,SE,,,,
City,Dresden,Dresden,DE,,,,
City,Missoula,Missoula,US,,,,
University,UvA,UvA,,,,,
University,TU Berlin,TU Berlin,,,,,
University,UCL,UCL,,,,,
Company,Neo4j,Neo4j,,,,,
Company,Acme,Acme,,,,,
Tag,graphs,graphs,,,,,
Tag,cypher,cypher,,,,,
Tag,rust,rust,,,,,
Person,Alice,Alice,,1,32,,
Person,Bob,Bob,,2,29,,
Person,Carol,Carol,,3,41,,
Person,Dave,Dave,,4,27,,
Person,Eve,Eve,,5,35,,
Person,Frank,Frank,,6,24,,
Person,Oskar,Oskar,,7,35,,
Person,Michael,Michael,,8,35,,
Person,Tomas,Tomas,,9,35,,
Person,Niels,Niels,,10,35,,
Person,Will,Will,,11,35,,
Post,101,,,101,,Learning Cypher,2024-01-10
Post,102,,,102,,Rust for graphs,2024-02-14
Post,103,,,103,,Reply: nice!,2024-02-15
Post,104,,,104,,Traversals 101,2024-03-01
`;

const RELS_CSV = `fromLabel,fromKey,toLabel,toKey,type,role,since,year
Person,Alice,City,Amsterdam,LIVES_IN,,,
Person,Bob,City,Amsterdam,LIVES_IN,,,
Person,Eve,City,Amsterdam,LIVES_IN,,,
Person,Carol,City,Berlin,LIVES_IN,,,
Person,Frank,City,Berlin,LIVES_IN,,,
Person,Dave,City,London,LIVES_IN,,,
Person,Oskar,City,Linköping,LIVES_IN,,,
Person,Tomas,City,Göteborg,LIVES_IN,,,
Person,Michael,City,Dresden,LIVES_IN,,,
Person,Niels,City,Amsterdam,LIVES_IN,,,
Person,Will,City,Missoula,LIVES_IN,,,
Person,Alice,University,UvA,STUDIED_AT,,,2015
Person,Bob,University,UvA,STUDIED_AT,,,2016
Person,Eve,University,UvA,STUDIED_AT,,,2013
Person,Carol,University,TU Berlin,STUDIED_AT,,,2005
Person,Frank,University,TU Berlin,STUDIED_AT,,,2020
Person,Dave,University,UCL,STUDIED_AT,,,2019
Person,Alice,Company,Neo4j,WORKS_AT,Engineer,2019,
Person,Bob,Company,Neo4j,WORKS_AT,Designer,2020,
Person,Eve,Company,Neo4j,WORKS_AT,PM,2021,
Person,Carol,Company,Acme,WORKS_AT,Analyst,2015,
Person,Dave,Company,Acme,WORKS_AT,Founder,2018,
Person,Oskar,Company,Neo4j,WORKS_AT,Engineer,2021,
Person,Michael,Company,Neo4j,WORKS_AT,Engineer,2022,
Person,Tomas,Company,Neo4j,WORKS_AT,Designer,2020,
Person,Niels,Company,Neo4j,WORKS_AT,PM,2023,
Person,Will,Company,Neo4j,WORKS_AT,Engineer,2024,
Person,Alice,Person,Bob,KNOWS,,2015,
Person,Bob,Person,Eve,KNOWS,,2016,
Person,Eve,Person,Carol,KNOWS,,2018,
Person,Carol,Person,Dave,KNOWS,,2020,
Person,Dave,Person,Frank,KNOWS,,2021,
Person,Alice,Person,Eve,KNOWS,,2017,
Person,Oskar,Person,Michael,KNOWS,,2021,
Person,Oskar,Person,Tomas,KNOWS,,2021,
Person,Oskar,Person,Niels,KNOWS,,2022,
Person,Oskar,Person,Will,KNOWS,,2022,
Person,Michael,Person,Tomas,KNOWS,,2020,
Person,Michael,Person,Niels,KNOWS,,2021,
Person,Michael,Person,Will,KNOWS,,2023,
Person,Tomas,Person,Niels,KNOWS,,2022,
Person,Tomas,Person,Will,KNOWS,,2023,
Person,Niels,Person,Will,KNOWS,,2024,
Person,Alice,Post,101,AUTHORED,,,
Person,Bob,Post,102,AUTHORED,,,
Person,Carol,Post,103,AUTHORED,,,
Person,Eve,Post,104,AUTHORED,,,
Post,101,Tag,cypher,HAS_TAG,,,
Post,101,Tag,graphs,HAS_TAG,,,
Post,102,Tag,rust,HAS_TAG,,,
Post,102,Tag,graphs,HAS_TAG,,,
Post,104,Tag,graphs,HAS_TAG,,,
Post,103,Post,102,REPLY_OF,,,
Person,Bob,Post,101,LIKES,,,
Person,Eve,Post,101,LIKES,,,
Person,Dave,Post,102,LIKES,,,
Person,Alice,Post,104,LIKES,,,
Person,Carol,Post,104,LIKES,,,
Person,Oskar,Post,102,LIKES,,,
Person,Michael,Post,102,LIKES,,,
Person,Tomas,Post,102,LIKES,,,
Person,Niels,Post,102,LIKES,,,
Person,Will,Post,102,LIKES,,,
`;

// Shared column metadata drives both the display script and seed parameters.
const INT_COLUMNS: ReadonlySet<string> = new Set(['id', 'age', 'since', 'year']);
const NODE_CSV = { label: 'label', key: 'key' };
const REL_CSV = {
  fromLabel: 'fromLabel',
  fromKey: 'fromKey',
  toLabel: 'toLabel',
  toKey: 'toKey',
  type: 'type',
};
const NODE_ROUTING_COLUMNS = new Set(Object.values(NODE_CSV));
const REL_ROUTING_COLUMNS = new Set(Object.values(REL_CSV));

// Simplified (illustrative) Cypher script to help users understand. Not real executed Cypher.
export const SEED_DISPLAY_SCRIPT = `
LOAD CSV WITH HEADERS FROM 'nodes.csv' AS row
CREATE (n:$(row.label))
SET n.name = row.name,
    n.country = row.country,
    n.id = toIntegerOrNull(row.id),
    n.age = toIntegerOrNull(row.age),
    n.content = row.content,
    n.createdAt = row.createdAt;

LOAD CSV WITH HEADERS FROM 'rels.csv' AS row
MATCH (a:$(row.fromLabel) {key: row.fromKey})
MATCH (b:$(row.toLabel) {key: row.toKey})
CREATE (a)-[r:$(row.type)]->(b)
SET r.role = row.role,
    r.since = toIntegerOrNull(row.since),
    r.year = toIntegerOrNull(row.year);
`.trim();

// `__key` is a temporary lookup property set on every node so the rels pass
// can MATCH by it. It is dropped at the end of the script.
export const SEED_SCRIPT = `
UNWIND $nodes AS row
CREATE (n:$(row.label))
SET n = row.props;

UNWIND $rels AS row
MATCH (a:$(row.fromLabel) {__key: row.fromKey})
MATCH (b:$(row.toLabel)   {__key: row.toKey})
CREATE (a)-[r:$(row.type)]->(b)
SET r = row.props;

MATCH (n) REMOVE n.__key;
`.trim();

// A tiny RFC-4180-ish CSV parser (no quoted fields with commas; our data has
// none). A row with a wrong column count is a dataset bug — fail loud instead
// of silently padding/truncating.
function parseCsv(text: string): Record<string, string>[] {
  const lines = text.trim().split('\n');
  const headers = csvHeaders(text);
  return lines.slice(1).map((line, i) => {
    const cells = line.split(',');
    if (cells.length !== headers.length) {
      throw new Error(
        `seed CSV row ${i + 2} has ${cells.length} cells but header has ${headers.length}`,
      );
    }
    return Object.fromEntries(headers.map((h, j) => [h, cells[j]]));
  });
}

function csvHeaders(text: string): string[] {
  return text.trim().split('\n')[0].split(',');
}

function withoutColumns(
  row: Record<string, string>,
  columns: ReadonlySet<string>,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(row).filter(([column]) => !columns.has(column)),
  );
}

// Everything except the routing columns, with INT_COLUMNS values coerced to
// numbers and blanks dropped. A non-integer value in an INT_COLUMNS cell is a
// dataset typo — throw so the module fails to load loudly instead of silently
// dropping the property (JSON.stringify emits null for NaN, and `SET n =
// row.props` would then skip it).
function propsFromRow(row: Record<string, string>): Record<string, string | number> {
  const props: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(row)) {
    if (v === '') continue;
    if (INT_COLUMNS.has(k)) {
      const n = Number(v);
      if (!Number.isInteger(n)) {
        throw new Error(`seed CSV: column "${k}" value "${v}" is not an integer`);
      }
      props[k] = n;
    } else {
      props[k] = v;
    }
  }
  return props;
}

// Pre-computed once per module load: SEED_SCRIPT just UNWINDs it.
export const SEED_PARAMS: string = JSON.stringify({
  nodes: parseCsv(NODES_CSV).map((row) => {
    return {
      label: row[NODE_CSV.label],
      props: {
        ...propsFromRow(withoutColumns(row, NODE_ROUTING_COLUMNS)),
        __key: row[NODE_CSV.key],
      },
    };
  }),
  rels: parseCsv(RELS_CSV).map((row) => {
    return {
      fromLabel: row[REL_CSV.fromLabel],
      fromKey: row[REL_CSV.fromKey],
      toLabel: row[REL_CSV.toLabel],
      toKey: row[REL_CSV.toKey],
      type: row[REL_CSV.type],
      props: propsFromRow(withoutColumns(row, REL_ROUTING_COLUMNS)),
    };
  }),
});

// A full-graph preview rendered after the seed completes.
export const PREVIEW_QUERY = templates.labeled(
  'preview entire graph',
  `MATCH (n)-[e]->(m)
RETURN n,e,m`,
);
