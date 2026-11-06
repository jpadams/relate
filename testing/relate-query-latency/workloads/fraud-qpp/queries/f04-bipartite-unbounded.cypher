// f04: f03 without the {1,6} cap — the unbounded + form
// source: IN-HOUSE, re-authored
// status: kept — does not terminate today (no walk budget, no frontier
// kernel); frontier-NFA acceptance case (LABS-644) and the walk-budget
// case (LABS-660 item 3)
MATCH p = SIMPLE (a:Account)<-[:FROM]-(first:Transaction)
  ((ti:Transaction)-[:TO]->(ai:Account)<-[:FROM]-(tj:Transaction)
     WHERE ti.date < tj.date
       AND ti.amount >= tj.amount >= 0.80 * ti.amount){1,}
  (last:Transaction)-[:TO]->(a)
RETURN a.id AS account,
       [n IN nodes(p) WHERE n:Transaction | n.amount] AS amounts
