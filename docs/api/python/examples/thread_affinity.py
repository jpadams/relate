"""Thread affinity: connections are thread-affine, and violating that is typed.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/thread_affinity.py

`relate.threadsafety` is 1 — the module is shareable between threads, a
connection is not. Two rules (GH #352 / LABS-700):

- using a connection on a thread other than the one that opened it raises
  `ProgrammingError` (catchable, and not fatal for the owning thread);
- dropping the last reference on another thread is safe — the deferred
  cleanup runs from that thread, no unraisable exception.

One connection per thread is the pattern for `:memory:` stores.
"""

import threading
from typing import Optional

import relate


def main():
    # The owning thread creates, uses, and closes its connection.
    conn = relate.connect(":memory:")
    conn.execute("CREATE (:Person {name: 'Alice'})")

    # A different thread is refused with a typed, catchable error.
    refusal: Optional[str] = None

    def on_worker_thread():
        nonlocal refusal
        try:
            conn.execute("MATCH (p:Person) RETURN p")
        except relate.ProgrammingError as e:
            refusal = str(e)

    worker = threading.Thread(target=on_worker_thread)
    worker.start()
    worker.join(timeout=30)
    assert not worker.is_alive(), "worker thread hung"
    assert refusal is not None, "cross-thread use should have been refused"
    print(f"refused on the worker thread: {refusal}")

    # The refusal is not fatal: the owning thread keeps working.
    count = conn.execute("MATCH (p:Person) RETURN count(p) AS c").fetchall()[0]["c"]
    assert count == 1
    print(f"owning thread still works: {count} person")

    # Per-thread connections on `:memory:` stores run concurrently.
    errors = []

    def own_connection(i):
        try:
            with relate.connect(":memory:") as c:
                c.executemany("CREATE (:N {v: $v})", [{"v": j} for j in range(50)])
                rows = c.execute("MATCH (n:N) RETURN count(n) AS c").fetchall()
                assert rows[0]["c"] == 50
        except Exception as e:  # pragma: no cover - surfaced via assert below
            errors.append((i, repr(e)))

    threads = [threading.Thread(target=own_connection, args=(i,)) for i in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=30)
    assert not any(t.is_alive() for t in threads), "a worker thread hung"
    assert not errors, f"per-thread connections failed: {errors}"
    print("4 per-thread connections completed concurrently")

    conn.close()

    # Releasing the last reference on another thread is safe (deferred cleanup).
    holder = [relate.connect(":memory:")]
    dropper = threading.Thread(target=holder.clear)
    dropper.start()
    dropper.join(timeout=30)
    assert not dropper.is_alive(), "dropper thread hung"
    print("foreign-thread drop: safe, no unraisable exception")


if __name__ == "__main__":
    main()
