// f03: bipartite fraud ring — chronologically ordered transactions, <=20% amount
// decay per hop, 2-6 ring members, bounded {1,6} quantified group with inline
// WHERE pruning (the allReduce shape LABS-642 item 1 inlines)
// source: IN-HOUSE, re-authored over the planned bipartite generator
// ((Account)-[:FROM]->(:Transaction)-[:TO]->(Account), planted rings with
// dates increasing and amounts decaying <=20%; see oracle.json)
// status: kept — the bipartite generator (FROM/TO transactions with planted
// decay rings) is not in this change; frontier-NFA acceptance case once it
// lands (LABS-644 family). The 8-ring spike reference point is recorded in
// oracle.json's _source
MATCH p = SIMPLE (a:Account)<-[:FROM]-(first:Transaction)
  ((ti:Transaction)-[:TO]->(ai:Account)<-[:FROM]-(tj:Transaction)
     WHERE ti.date < tj.date
       AND ti.amount >= tj.amount >= 0.80 * ti.amount){1,6}
  (last:Transaction)-[:TO]->(a)
RETURN a.id AS account,
       [n IN nodes(p) WHERE n:Transaction | n.amount] AS amounts
