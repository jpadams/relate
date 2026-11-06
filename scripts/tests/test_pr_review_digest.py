"""Tests for pr_review_digest.py — the newest-first review-comment digest.

Covers the AGENTS.md "read review comments incrementally, newest first"
contract: reverse (created_at, id) ordering, the id tie-break for
same-second bursts, `superseded` marking (only the author's newest
comment is current), `--since-id` incremental filtering, `--limit`, the
`last-comment-id` footer, and fail-loud behavior on malformed entries —
the single `normalize` pass rejects a non-dict entry, a missing key, a
wrong-typed field, an explicit-null body, and a bool masquerading as an
int id. No silent fallback over any of them.
"""

import unittest
from pathlib import Path

from script_loader import load

SUBJECT = load(
    "pr_review_digest", Path(__file__).resolve().parents[2] / "scripts" / "pr_review_digest.py"
)
DigestError = SUBJECT.DigestError
digest_lines = SUBJECT.digest_lines
normalize = SUBJECT.normalize
to_comments = SUBJECT.to_comments

LONG_BODY = "\n".join(["x"] * 50)


def comment(cid, login, created, body="verdict: pass"):
    return {
        "id": cid,
        "user": {"login": login},
        "created_at": created,
        "body": body,
    }


def render(comments, **kwargs):
    """digest_lines over a raw payload — the to_comments + digest_lines chain."""
    return digest_lines(to_comments(comments), **kwargs)


def comment_headers(lines):
    """The block headers, in output order — the one selection site."""
    return [l for l in lines if l.startswith("--- comment")]


def header_ids(lines):
    """The comment ids in output order, derived from the headers."""
    return [int(h.split()[2]) for h in comment_headers(lines)]


class OrderingTests(unittest.TestCase):
    def test_newest_first(self):
        comments = [
            comment(1, "stellar", "2026-10-29T10:00:00Z"),
            comment(2, "stellar", "2026-10-29T11:00:00Z"),
            comment(3, "rust", "2026-10-29T12:00:00Z"),
        ]
        self.assertEqual(header_ids(render(comments)), [3, 2, 1])

    def test_same_second_tie_breaks_on_id(self):
        comments = [
            comment(10, "stellar", "2026-10-29T10:00:00Z"),
            comment(11, "stellar", "2026-10-29T10:00:00Z"),
        ]
        self.assertEqual(header_ids(render(comments)), [11, 10])

    def test_superseded_marks_all_but_authors_newest(self):
        comments = [
            comment(1, "stellar", "2026-10-29T10:00:00Z"),
            comment(2, "stellar", "2026-10-29T11:00:00Z"),
            comment(3, "rust", "2026-10-29T09:00:00Z"),
        ]
        headers = comment_headers(render(comments))
        self.assertNotIn("superseded", headers[0])  # stellar id 2, author's newest
        self.assertIn("superseded", headers[1])  # stellar id 1, superseded
        self.assertNotIn("superseded", headers[2])  # rust id 3, only comment


class FilterTests(unittest.TestCase):
    def test_since_id_is_strictly_greater(self):
        comments = [
            comment(1, "a", "2026-10-29T10:00:00Z"),
            comment(2, "b", "2026-10-29T11:00:00Z"),
        ]
        lines = render(comments, since_id=1)
        self.assertEqual(header_ids(lines), [2])

    def test_limit_caps_blocks_after_filtering(self):
        comments = [
            comment(i, f"a{i}", f"2026-10-29T1{i}:00:00Z") for i in range(5)
        ]
        lines = render(comments, limit=2)
        self.assertEqual(header_ids(lines), [4, 3])

    def test_last_comment_id_is_global_max_even_when_filtered(self):
        comments = [
            comment(1, "a", "2026-10-29T10:00:00Z"),
            comment(7, "b", "2026-10-29T11:00:00Z"),
        ]
        lines = render(comments, since_id=5)
        self.assertIn("last-comment-id: 7", lines)

    def test_empty_comment_list(self):
        self.assertEqual(digest_lines([]), ["last-comment-id: 0"])


class BodyTests(unittest.TestCase):
    def test_body_truncated_with_more_lines_marker(self):
        comments = [comment(1, "a", "2026-10-29T10:00:00Z", body=LONG_BODY)]
        self.assertIn("… (10 more lines, --full)", render(comments))

    def test_full_prints_whole_body(self):
        comments = [comment(1, "a", "2026-10-29T10:00:00Z", body=LONG_BODY)]
        lines = render(comments, full=True)
        self.assertNotIn("… (10 more lines, --full)", lines)
        self.assertEqual(sum(1 for l in lines if l == "x"), 50)


class MalformedTests(unittest.TestCase):
    """The typed boundary — `normalize` — called directly, as
    `fetch_comments` calls it: every malformed shape exits through the
    one DigestError path."""

    def test_non_list_payload_fails_loud(self):
        with self.assertRaises(DigestError):
            to_comments({"not": "a list"})

    def test_non_dict_entry_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize("not-a-comment")

    def test_missing_id_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"user": {"login": "a"}, "created_at": "t", "body": "x"})

    def test_non_integer_id_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize(comment("one", "a", "2026-10-29T10:00:00Z"))

    def test_bool_id_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize(comment(True, "a", "2026-10-29T10:00:00Z"))

    def test_missing_user_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"id": 1, "created_at": "t", "body": "x"})

    def test_missing_login_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"id": 1, "user": {}, "created_at": "t", "body": "x"})

    def test_missing_body_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"id": 1, "user": {"login": "a"}, "created_at": "t"})

    def test_null_body_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"id": 1, "user": {"login": "a"}, "created_at": "t", "body": None})

    def test_non_string_created_at_fails_loud(self):
        with self.assertRaises(DigestError):
            normalize({"id": 1, "user": {"login": "a"}, "created_at": 42, "body": "x"})


if __name__ == "__main__":
    unittest.main()
