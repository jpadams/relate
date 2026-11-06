#!/usr/bin/env python3
"""Materialize the Stack Overflow demo store for the expand-workload skill.

Fetches the 100 latest questions (with answers) for a tag from the public
Stack Exchange API and MERGEs them into a relate store at the given path:

    python3 build_so_store.py target/expand-workload/so.db [--tag neo4j] [--pages 1]

Network access is required (Stack Exchange API); nightly CI is the intended
caller. Prints node/relationship counts so the log shows what was loaded.
"""

import argparse
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

BASE = "https://api.stackexchange.com/2.3/questions"
FILTER = "filter=!5-i6Zw8Y)4W7vpy91PMYsKM-k9yzEsSC1_Uxlf"
API = (
    BASE
    + "?page={page}&pagesize=100&order=desc&sort=creation&tagged={tag}&site=stackoverflow&"
    + FILTER
)


def _fetch(url, retries=4):
    """GET the items array, with backoff only on the typed throttle answer
    (HTTP 429) — any other failure propagates."""
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                return json.loads(r.read().decode())["items"]
        except urllib.error.HTTPError as e:
            if e.code != 429 or attempt == retries - 1:
                raise
            time.sleep(2**attempt + 0.4)


def _counts(db):
    """Node/relationship counts — the one place this pair exists."""
    nodes = db.execute("MATCH (n) RETURN count(n) AS c").fetchall()[0]["c"]
    rels = db.execute("MATCH ()-[r]->() RETURN count(r) AS c").fetchall()[0]["c"]
    return nodes, rels


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("store", type=Path)
    ap.add_argument("--tag", default="neo4j")
    ap.add_argument("--pages", type=int, default=1)
    ap.add_argument(
        "--enrich",
        action="store_true",
        help="grow the store with real domain data: comments embedded in the "
        "questions payload, and the related ring — /questions/{id}/related per "
        "loaded question, then again for the top ring stubs (two levels)",
    )
    args = ap.parse_args()

    try:
        import relate
    except ImportError:
        raise SystemExit("needs the relate Python package (pip install ./surfaces/relate-python)")

    db = relate.connect(str(args.store))
    total = 0
    for page in range(1, args.pages + 1):
        questions = _fetch(API.format(tag=args.tag, page=page))
        total += len(questions)
        db.execute(
            """UNWIND $questions AS q MERGE (question:Question {uuid: q.question_id})
               ON CREATE SET question.title=q.title, question.score=q.score,
                 question.view_count=q.view_count, question.answer_count=q.answer_count,
                 question.is_answered=q.is_answered""",
            {"questions": questions},
        )
        db.execute(
            """UNWIND $questions AS q
               MERGE (owner:User {uuid: coalesce(q.owner.user_id, 'deleted')})
                 ON CREATE SET owner.display_name=coalesce(q.owner.display_name,'deleted')
               MATCH (question:Question {uuid: q.question_id})
               MERGE (owner)-[:ASKED]->(question)""",
            {"questions": questions},
        )
        db.execute(
            """UNWIND $questions AS q
               MATCH (question:Question {uuid: q.question_id})
               UNWIND q.tags AS t
               MERGE (tag:Tag {name: t})
               MERGE (question)-[:TAGGED]->(tag)""",
            {"questions": questions},
        )
        db.execute(
            """UNWIND $questions AS q
               MATCH (question:Question {uuid: q.question_id})
               UNWIND q.answers AS a
               MERGE (answer:Answer {uuid: a.answer_id})
                 ON CREATE SET answer.score=a.score, answer.is_accepted=a.is_accepted
               MERGE (answer)-[:ANSWERED]->(question)
               MERGE (answerer:User {uuid: coalesce(a.owner.user_id,'deleted')})
                 ON CREATE SET answerer.display_name=coalesce(a.owner.display_name,'deleted')
               MERGE (answerer)-[:PROVIDED]->(answer)""",
            {"questions": questions},
        )
    nodes, rels = _counts(db)
    print(f"{total} questions fetched; store at {args.store}: {nodes} nodes, {rels} relationships")

    if args.enrich:
        enrich(db)
        nodes, rels = _counts(db)
        print(f"enriched: {nodes} nodes, {rels} relationships")
    db.close()


def _related_ring(db, source_ids, depth):
    """MERGE (source)-[:RELATED]->(target) in one UNWIND batch, plus ring
    stubs for unseen targets. Returns {stub_uuid: score} — the caller picks
    the ring-2 seeds from it instead of re-deriving the ring with a query."""
    rows = []
    stubs = {}
    for i, qid in enumerate(source_ids):
        for it in _fetch(f"{BASE}/{qid}/related?pagesize=100&site=stackoverflow&{FILTER}"):
            rows.append(
                {
                    "source": qid,
                    "target": it["question_id"],
                    "title": it.get("title"),
                    "score": it.get("score", 0),
                    "views": it.get("view_count", 0),
                    "answered": it.get("is_answered", False),
                    "answers": it.get("answer_count", 0),
                    "link": it.get("link"),
                    "depth": depth,
                }
            )
            if it["question_id"] not in source_ids:
                stubs[it["question_id"]] = it.get("score", 0)
        if i % 20 == 19:
            time.sleep(0.4)
            print(f"  {i + 1}/{len(source_ids)} questions…")
    if rows:
        db.execute(
            """UNWIND $rows AS r
               MATCH (q:Question {uuid: r.source})
               MERGE (t:Question {uuid: r.target})
                 ON CREATE SET t.title = r.title, t.score = r.score,
                   t.view_count = r.views, t.is_answered = r.answered,
                   t.answer_count = r.answers, t.link = r.link, t.depth = r.depth
               MERGE (q)-[:RELATED]->(t)""",
            {"rows": rows},
        )
    print(f"depth-{depth} ring: {len(rows)} RELATED edges, {len(stubs)} new stubs")
    return stubs


def _merge_comments(db, questions):
    """All comments in one UNWIND batch — the idiom main() uses for questions."""
    rows = [
        {
            "id": c["comment_id"],
            "score": c.get("score", 0),
            "created": c.get("creation_date"),
            "link": c.get("link"),
            "edited": c.get("edited", False),
            "qid": q["question_id"],
            "ouuid": (c.get("owner") or {}).get("user_id", "deleted"),
            "oname": (c.get("owner") or {}).get("display_name", "deleted"),
        }
        for q in questions
        for c in q.get("comments", [])
    ]
    if rows:
        db.execute(
            """UNWIND $rows AS c
               MERGE (comment:Comment {uuid: c.id})
                 ON CREATE SET comment.score = c.score, comment.creation_date = c.created,
                   comment.link = c.link, comment.edited = c.edited
               WITH comment, c
               MATCH (q:Question {uuid: c.qid})
               MERGE (comment)-[:COMMENTED_ON]->(q)
               MERGE (owner:User {uuid: c.ouuid})
                 ON CREATE SET owner.display_name = c.oname
               MERGE (owner)-[:COMMENTED]->(comment)""",
            {"rows": rows},
        )
    return len(rows)


def enrich(db):
    """Real domain growth: comments on the loaded questions, then the
    related ring two levels deep (loaded → ring-1 stubs → ring-2 targets)."""
    ids = [r["uuid"] for r in db.execute("MATCH (q:Question) RETURN q.uuid AS uuid").fetchall()]

    questions = _fetch(f"{BASE}/{';'.join(map(str, ids))}?pagesize=100&site=stackoverflow&{FILTER}")
    print(f"{_merge_comments(db, questions)} comments merged")

    ring1 = _related_ring(db, ids, 1)
    top_stubs = [
        qid for qid, _ in sorted(ring1.items(), key=lambda kv: kv[1], reverse=True)[:60]
    ]
    _related_ring(db, top_stubs, 2)


if __name__ == "__main__":
    main()
