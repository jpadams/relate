// The f06 sample fixture: one designed 4-account ring plus five decoys.
//
// The ring: accounts 1-2-3-4, transactions t1-t4 with amounts decaying
// exactly 10% per hop (1000 -> 900 -> 810 -> 729, every hop inside the
// 20% rule) and dates one day apart in ring order. A transaction's FROM
// edge names the account the money leaves, its TO edge the account it
// arrives at.
//
// The decoys, each excluded for one reason:
// - d1 (amount 100, after t1): breaks the decay rule (100 < 0.80 * 1000)
// - d2 (amount 850, before t2's date): breaks the date-order rule
// - d3 (amount 500, after t3): breaks the decay rule (500 < 0.80 * 810)
// - t7 (amount 800, after t3): follows both rules but cannot close the
//   ring — its destination, account 5, has no transaction back to 1
// - account 5 itself: no further edges, pure noise
//
// IN-HOUSE (this workload's oracle and fixture are one designed pair);
// see the workload README for the suite's provenance.
CREATE (a1:Account {a_id: 1}), (a2:Account {a_id: 2}), (a3:Account {a_id: 3}),
       (a4:Account {a_id: 4}), (a5:Account {a_id: 5}),
       (t1:Transaction {t_id: 1, amount: 1000, date: date('2026-01-01')})-[:FROM]->(a1),
       (t1)-[:TO]->(a2),
       (t2:Transaction {t_id: 2, amount: 900, date: date('2026-01-02')})-[:FROM]->(a2),
       (t2)-[:TO]->(a3),
       (t3:Transaction {t_id: 3, amount: 810, date: date('2026-01-03')})-[:FROM]->(a3),
       (t3)-[:TO]->(a4),
       (t4:Transaction {t_id: 4, amount: 729, date: date('2026-01-04')})-[:FROM]->(a4),
       (t4)-[:TO]->(a1),
       (d1:Transaction {t_id: 11, amount: 100, date: date('2026-01-05')})-[:FROM]->(a2),
       (d1)-[:TO]->(a4),
       (d2:Transaction {t_id: 12, amount: 850, date: date('2025-12-31')})-[:FROM]->(a3),
       (d2)-[:TO]->(a1),
       (d3:Transaction {t_id: 13, amount: 500, date: date('2026-01-06')})-[:FROM]->(a4),
       (d3)-[:TO]->(a2),
       (t7:Transaction {t_id: 14, amount: 800, date: date('2026-01-05')})-[:FROM]->(a4),
       (t7)-[:TO]->(a5)
