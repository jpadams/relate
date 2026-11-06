#!/usr/bin/env python3
"""Stub `gh` for update-pr-comment.sh tests — never touches the network.

The test harness puts this on PATH as `gh` (a tiny /bin/sh shim). All
state lives in $GH_STUB_STATE: `comments.json` (the PR conversation's
issue comments) and `calls.log` (one JSON line per `gh api` call, with
the endpoint, method, --jq filter, --paginate flag, and the request body
for writes). POSTed comments are appended to `comments.json` so a later
invocation of the updater finds them, exactly like the real API — that
evolving state is what proves a rerun updates in place instead of
duplicating.

Served surface (the updater's full gh contract):

  gh api --paginate --jq FILTER repos/{o}/{r}/issues/{n}/comments      (GET)
  gh api --method PATCH -F body=@FILE repos/{o}/{r}/issues/comments/{id}
  gh api --method POST  -F body=@FILE repos/{o}/{r}/issues/{n}/comments

The GET branch emulates the one --jq shape the updater uses —

  .[] | select(.user.login == "AUTHOR") |
  select((.body // "") | contains("NEEDLE")) | .id

— printing one matching id per line: a comment matches only when its
author is the filter's author AND its body carries the needle (real gh
applies the filter per page and concatenates). `comments.json` entries
therefore carry the real API's author shape, `"user": {"login": ...}`,
and a POST is attributed to `github-actions[bot]` — exactly the login
the real API records for a GITHUB_TOKEN comment — so a rerun finds and
PATCHes the comment it created even under the updater's author filter.
Anything unexpected exits 3 so the tests catch contract drift instead
of silently passing; a PATCH whose id matches no comment in the state
also exits 3 (the updater must only ever PATCH an id the list call
returned).

GH_STUB_MODE=fail fails every call; GH_STUB_MODE=fail_write fails only
PATCH/POST. Both exercise the updater's best-effort paths.
"""

import json
import os
import re
import sys

LIST_RE = re.compile(r"^repos/[^/]+/[^/]+/issues/(\d+)/comments$")
COMMENT_RE = re.compile(r"^repos/[^/]+/[^/]+/issues/comments/(\d+)$")
NEEDLE_RE = re.compile(r"contains\(['\"](.+?)['\"]\)")
AUTHOR_RE = re.compile(r"\.user\.login\s*==\s*['\"](.+?)['\"]\)")
# The login the real API attributes to a comment posted with the
# workflow's GITHUB_TOKEN — the same identity update-pr-comment.sh
# filters its marker search on.
POST_AUTHOR = "github-actions[bot]"

argv = sys.argv[1:]
if argv and argv[0] == "api":
    argv = argv[1:]
else:
    sys.stderr.write("gh-stub: only the `gh api` subcommand is stubbed\n")
    sys.exit(3)

endpoint = None
method = None
jq = None
paginate = False
fields = {}
i = 0
while i < len(argv):
    arg = argv[i]
    if arg == "--paginate":
        paginate = True
    elif arg in ("-X", "--method"):
        i += 1
        method = argv[i]
    elif arg in ("-q", "--jq"):
        i += 1
        jq = argv[i]
    elif arg in ("-F", "-f"):
        i += 1
        key, _, value = argv[i].partition("=")
        fields[key] = value
    elif arg in ("--silent", "-i", "--include"):
        pass
    elif arg == "--input":
        i += 1  # value ignored; the updater never uses --input
    elif arg.startswith("-"):
        sys.stderr.write(f"gh-stub: unexpected flag {arg}\n")
        sys.exit(3)
    elif endpoint is None:
        endpoint = arg
    else:
        sys.stderr.write(f"gh-stub: unexpected argument {arg}\n")
        sys.exit(3)
    i += 1

if method is None:
    # gh switches to POST automatically when request fields are present.
    method = "POST" if fields else "GET"

state = os.environ.get("GH_STUB_STATE")
if state is None:
    sys.stderr.write("gh-stub: GH_STUB_STATE is not set\n")
    sys.exit(3)
mode = os.environ.get("GH_STUB_MODE", "ok")

if mode == "fail" or (mode == "fail_write" and method != "GET"):
    sys.stderr.write("gh-stub: simulated network failure\n")
    sys.exit(1)


def comments_path():
    return os.path.join(state, "comments.json")


def read_comments():
    if not os.path.exists(comments_path()):
        return []
    with open(comments_path()) as handle:
        return json.load(handle)


def write_comments(comments):
    with open(comments_path(), "w") as handle:
        json.dump(comments, handle)


def log_call(body):
    entry = {
        "method": method,
        "endpoint": endpoint,
        "jq": jq,
        "paginate": paginate,
        "body": body,
    }
    with open(os.path.join(state, "calls.log"), "a") as handle:
        handle.write(json.dumps(entry) + "\n")


def body_from_fields():
    ref = fields.get("body")
    if ref is None or not ref.startswith("@"):
        sys.stderr.write("gh-stub: expected -F body=@FILE\n")
        sys.exit(3)
    with open(ref[1:]) as handle:
        return handle.read()


if method == "GET":
    if jq is None:
        sys.stderr.write("gh-stub: GET without --jq is not stubbed\n")
        sys.exit(3)
    if LIST_RE.match(endpoint or "") is None:
        sys.stderr.write(f"gh-stub: unexpected GET endpoint {endpoint}\n")
        sys.exit(3)
    needle = NEEDLE_RE.search(jq)
    author = AUTHOR_RE.search(jq)
    if needle is None or author is None:
        sys.stderr.write(f"gh-stub: unsupported --jq filter {jq}\n")
        sys.exit(3)
    log_call(None)
    for comment in read_comments():
        user = comment.get("user") or {}
        if user.get("login") != author.group(1):
            continue
        if needle.group(1) in (comment.get("body") or ""):
            print(comment["id"])
    sys.exit(0)

if method == "PATCH":
    match = COMMENT_RE.match(endpoint or "")
    if match is None:
        sys.stderr.write(f"gh-stub: unexpected PATCH endpoint {endpoint}\n")
        sys.exit(3)
    new_body = body_from_fields()
    comment_id = int(match.group(1))
    comments = read_comments()
    target = None
    for comment in comments:
        if comment["id"] == comment_id:
            target = comment
    if target is None:
        # Drift policy: the updater PATCHes only ids its (author-filtered)
        # list call returned, so an unknown id is contract drift, not a
        # state the real API can produce through this script.
        sys.stderr.write(f"gh-stub: PATCH of unknown comment id {comment_id}\n")
        sys.exit(3)
    log_call(new_body)
    target["body"] = new_body
    write_comments(comments)
    print("{}")
    sys.exit(0)

if method == "POST":
    if LIST_RE.match(endpoint or "") is None:
        sys.stderr.write(f"gh-stub: unexpected POST endpoint {endpoint}\n")
        sys.exit(3)
    new_body = body_from_fields()
    log_call(new_body)
    comments = read_comments()
    new_id = max((comment["id"] for comment in comments), default=0) + 1
    response = {"id": new_id, "body": new_body, "user": {"login": POST_AUTHOR}}
    comments.append(response)
    write_comments(comments)
    print(json.dumps(response))
    sys.exit(0)

sys.stderr.write(f"gh-stub: unexpected method {method}\n")
sys.exit(3)
