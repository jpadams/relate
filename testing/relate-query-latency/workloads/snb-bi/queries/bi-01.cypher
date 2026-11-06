// bi-01: Posting summary
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-1.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (12 rows, ~1.0 s — 2026-11-02 landing run, release, graph-benchmark-ldbc SF0.1)
MATCH (message:Comment|Post)
WHERE datetime(message.creationDate) < $datetime
WITH count(message) AS totalMessageCountInt
WITH toFloat(totalMessageCountInt) AS totalMessageCount
MATCH (message:Comment|Post)
WHERE datetime(message.creationDate) < $datetime
  AND message.content IS NOT NULL
WITH
  totalMessageCount,
  message,
  datetime(message.creationDate).year AS year
WITH
  totalMessageCount,
  year,
  message:Comment AS isComment,
  CASE
    WHEN message.length <  40 THEN 0
    WHEN message.length <  80 THEN 1
    WHEN message.length < 160 THEN 2
    ELSE                           3
  END AS lengthCategory,
  count(message) AS messageCount,
  sum(message.length) / toFloat(count(message)) AS averageMessageLength,
  sum(message.length) AS sumMessageLength
RETURN
  year,
  isComment,
  lengthCategory,
  messageCount,
  averageMessageLength,
  sumMessageLength,
  messageCount / totalMessageCount AS percentageOfMessages
ORDER BY
  year DESC,
  isComment ASC,
  lengthCategory ASC
