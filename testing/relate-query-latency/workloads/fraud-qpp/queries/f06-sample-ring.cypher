// f06: unit-scale fixture — the designed 4-account sample ring
// (1000 -> 900 -> 810 -> 729, dates one day apart) loaded by
// fixture-f06.cypher; every other transaction in the fixture breaks the
// decay or date rule, or follows the rules but cannot close the ring
// source: IN-HOUSE, re-authored (fixture and expected answer are both
// ours). The anchor is f03's first/last spelling: a two-relationship
// anchor through a single transaction does not express a ring (it
// demands a transaction that both leaves and enters the account).
// status: ok — answer-checked against oracle.json (rows check)
MATCH p = SIMPLE (a:Account {a_id: 1})<-[:FROM]-(first:Transaction)
  ((ti:Transaction)-[:TO]->(ai:Account)<-[:FROM]-(tj:Transaction)
     WHERE ti.date < tj.date
       AND ti.amount >= tj.amount >= 0.80 * ti.amount){1,6}
  (last:Transaction)-[:TO]->(a)
RETURN [n IN nodes(p) WHERE n:Account | n.a_id] AS ring,
       [n IN nodes(p) WHERE n:Transaction | n.amount] AS amounts
