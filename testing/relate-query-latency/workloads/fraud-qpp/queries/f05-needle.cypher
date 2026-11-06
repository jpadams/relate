// f05: the needle search — one planted long ring hidden under ~100k noise
// transactions; only the planted ring satisfies the ordering rule
// source: IN-HOUSE, re-authored (the planned generator plants a ring of
// accounts with id 'needle-<i>', test: true, dates one per day). The
// anchor is f03's first/last spelling, like f06.
// status: kept — the needle generator (a planted N-hop ring under
// uniform noise) is not in this change; frontier-NFA acceptance case
// once it lands (LABS-644 family)
MATCH p = SIMPLE (a:Account {id: 'needle-0'})<-[:FROM]-(first:Transaction {test: true})
  ((ti:Transaction {test: true})-[:TO]->(ai:Account {test: true})<-[:FROM]-(tj:Transaction {test: true})
     WHERE ti.date < tj.date){1,}
  (last:Transaction {test: true})-[:TO]->(a)
RETURN [n IN nodes(p) WHERE n:Account | n.id] AS ring
