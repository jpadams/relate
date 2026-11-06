(ns jepsen.relate.elle
  "The curated Elle anomaly set (LABS-652) as jepsen checkers.

  The whitelist mechanism is the consistency-model vector itself (elle
  has no :skip-checks option): elle's checkers default to
  [:strict-serializable], which proscribes G2/write skew — but SI
  permits G2, and Relate's ADRs deliberately ship SI (ADR-0015
  Alternatives: 'SSI / read-set validation — Rejected'; reaffirmed
  ADR-0028: 'SSI / write-skew detection is out of scope'). Checking the
  snapshot-isolation model family IS the whitelist: PL-SI proscribes
  [:internal :G1 :G-SI :G-nonadjacent :PL-SI-cycle-exists] and, via
  implied models, :lost-update and :G0 — but not :G2 or :G2-item.
  Strong-session-snapshot-isolation is asserted because one Bolt
  connection = one session and per-connection transactions are strictly
  ordered (ADR-0028).

  The counter and bank workloads read everything they write (read set ⊆
  write set), so any two concurrent transactions sharing a key have
  overlapping write sets and first-committer-wins (ADR-0015 §4) aborts
  one — no rw anti-dependency edge between committed transactions, no
  G2 cycle constructible, the SI history serializes in commit order.
  That is why those lanes additionally GATE on [:serializable]: the
  same history must pass it. [:strict-serializable] is discovery only —
  realtime plausibly holds (ack-after-apply_redo, snapshot pinned at
  BEGIN), but no ADR states it as a claim, so its verdict is recorded
  and never fails the run."
  (:require [elle [list-append :as list-append]
                    [rw-register :as rw-register]]
            [jepsen [checker :as checker]]))

(def si-models
  "The whitelist vector: raw SI plus its strong-session strengthening."
  [:snapshot-isolation :strong-session-snapshot-isolation])

(def serializable-models
  "The counter/bank gate: SI ≡ serializable by workload shape."
  [:serializable])

(def strict-serializable-models
  "Discovery only — never a gate."
  [:strict-serializable])

(defn model-check
  "Wraps an elle check fn (elle.list-append/check or
  elle.rw-register/check) and a consistency-model vector into a jepsen
  checker. `history-xform` reshapes the jepsen history into the op shape
  the elle namespace consumes — identity when the history's client ops
  already carry mop-vector :values, a derivation when the lane's ops are
  workload-shaped (the bank lane's :transfer maps).

  elle's error objects (duplicate appends, unexpected mop types) throw
  out of the check; jepsen's compose wraps every checker in check-safe,
  so a thrown client-discipline bug surfaces as :valid? :unknown — a
  red run — instead of a quiet pass."
  [check-fn models history-xform]
  (reify checker/Checker
    (check [_this _test history _opts]
      (assoc (check-fn {:consistency-models models} (history-xform history))
             :consistency-models models))))

(defn discovery-only
  "Rewraps a checker so its verdict is RECORDED but never fails the run:
  the [:strict-serializable] lane is discovery, not a gate. The real
  verdict rides :discovery-verdict; :valid? is forced true so compose
  propagates the gate verdicts alone."
  [chk]
  (reify checker/Checker
    (check [_this test history opts]
      (let [res (checker/check chk test history opts)]
        (assoc res
               :discovery-only? true
               :discovery-verdict (:valid? res)
               :valid? true)))))

(defn- lane-checkers
  "The elle checker triple every Elle lane runs over its own check fn
  and history transform: the SI whitelist gate, the [:serializable]
  workload-shape gate, and [:strict-serializable] as discovery only."
  [check-fn history-xform]
  {:elle-si
   (model-check check-fn si-models history-xform)

   :elle-serializable
   (model-check check-fn serializable-models history-xform)

   :elle-strict-serializable
   (discovery-only
     (model-check check-fn strict-serializable-models history-xform))})

(defn counter-checkers
  "The elle checker triple for the counter lane: list-append is the
  primary namespace (most powerful rules — version order inferred from
  unique appends), the history is consumed as-is (its ops are already
  mop-vector txns)."
  []
  (lane-checkers list-append/check identity))

(defn bank-checkers
  "The elle checker triple for the bank lane, over rw-registers (there
  is no elle.bank namespace — bank conservation and non-negativity are
  jepsen's own bank checker, wired in the bank workload)."
  [history-xform]
  (lane-checkers rw-register/check history-xform))
