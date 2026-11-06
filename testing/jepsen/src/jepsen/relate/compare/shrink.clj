(ns jepsen.relate.compare.shrink
  "Shrink candidates for the compare workload's op data (LABS-657):
  every candidate a shrinker enumerates is strictly smaller by `weight`,
  so the greedy loop in jepsen.relate.compare.diff/shrink-case!
  terminates without a pass budget. Candidates are DATA — the shrinker
  re-renders each through jepsen.relate.compare.render/render-stmt,
  the one canonical data->Cypher path.")

(defn weight
  "A structural size for shrink termination: every candidate a shrinker
  enumerates has strictly smaller weight, so the greedy loop in
  diff/shrink-case! terminates without a pass budget. Numbers shrink
  toward zero, strings toward \"a\" (the one minimal string — every
  other string, one-character ones included, weighs more, so the
  string shrink is strictly decreasing), collections by elements."
  [x]
  (cond
    (map? x)       (inc (transduce (map weight) + 0 (vals x)))
    (vector? x)    (+ 2 (transduce (map weight) + 0 x))
    (sequential? x) (+ 1 (transduce (map weight) + 0 x))
    (number? x)    (if (zero? x) 1 2)
    (string? x)    (if (= "a" x) 1 2)
    :else          1))

(def ^:private protected-keys
  "Map entries the literal shrinker must never touch: nid/rid are the
  user-visible identity the whole comparison keys on, and a range's
  bounds must stay a legal quantifier."
  #{:nid :rid :range})

(defn- shrink-lit
  "The smaller version of a literal value, or nil when the value is
  already minimal. Numbers go to 0, strings to \"a\", lists drop their
  last element. Keywords and booleans are already minimal."
  [v]
  (cond
    (and (number? v) (not (zero? v))) 0
    (and (string? v) (not= "a" v)) "a"
    (and (vector? v) (seq v)) (pop v)
    :else nil))

(defn lit-variants
  "Every variant of a data tree with exactly ONE literal replaced by its
  shrunk value. Structural markers are keywords (never shrunk); the only
  strings in op data are values — labels, names, string arguments — all
  safe to shrink."
  [x]
  (cond
    (map? x)
    (into []
          (mapcat (fn [[k v]]
                    (when-not (protected-keys k)
                      (concat
                        (when-let [v' (shrink-lit v)] [(assoc x k v')])
                        (map #(assoc x k %) (lit-variants v))))))
          x)
    (vector? x)
    (into []
          (mapcat (fn [i]
                    (let [v (nth x i)]
                      (concat
                        (when-let [v' (shrink-lit v)] [(assoc x i v')])
                        (map #(assoc x i %) (lit-variants v))))))
          (range (count x)))
    :else []))

(defn drop-nth
  "Vector v without its i'th element."
  [v i]
  (into (subvec v 0 i) (subvec v (inc i))))

(defn- indexed-variants
  "Every variant of vector v with the element at i replaced by one of
  (f element)."
  [v f]
  (into []
        (mapcat (fn [i] (map #(assoc v i %) (f (nth v i))))
                (range (count v)))))

(defn- setup-candidates
  "Smaller versions of a setup statement: drop optional properties, drop
  the secondary label. No literal shrinking here — setup values come
  from gen.clj's tiny vocabulary ('Alice', 0-9, 0.5), so literal
  variants cost far more shrink time than they ever save."
  [s]
  (case (:kind s)
    :create-node
    (concat
      (for [k (remove #{:nid} (keys (:props s)))]
        (update s :props dissoc k))
      (when (> (count (:labels s)) 1)
        [(update s :labels subvec 0 1)]))
    :create-rel
    (for [k (remove #{:rid} (keys (:props s)))]
      (update s :props dissoc k))
    []))

(defn- write-candidates
  "Smaller versions of a write op: drop SET/REMOVE items, drop the label
  mutation, simplify a literal. Guarded so the rendered statement always
  keeps at least one item after SET/REMOVE."
  [w]
  (case (:kind w)
    :set
    (concat
      (when (> (count (:sets w)) 1)
        (for [k (keys (:sets w))] (update w :sets dissoc k)))
      (when (seq (:add-labels w)) [(dissoc w :add-labels)])
      (lit-variants w))
    :remove
    (concat
      (when (> (count (:remove-props w)) 1)
        (for [i (range (count (:remove-props w)))]
          (update w :remove-props drop-nth i)))
      (when (seq (:remove-labels w)) [(dissoc w :remove-labels)])
      (lit-variants w))
    :delete
    []))

(defn- node-el-candidates [el]
  (when (= :node (:type el))
    (concat (when (seq (:labels el)) [(dissoc el :labels)])
            (when (:props el) [(dissoc el :props)]))))

(defn- chain-candidates [chain]
  (indexed-variants chain node-el-candidates))

(defn- clause-candidates [clause]
  (map #(assoc clause :groups %)
       (indexed-variants (:groups clause) chain-candidates)))

(defn- query-candidates
  "Smaller versions of a read query: drop WHERE predicates, RETURN
  items, ORDER BY/SKIP/LIMIT/DISTINCT, the WITH stage's filter, the
  OPTIONAL clause, a comma-group, or a node pattern's label/props
  constraint; plus one-literal simplification anywhere."
  [q]
  (concat
    (when-let [preds (get-in q [:where :preds])]
      (if (= 1 (count preds))
        [(dissoc q :where)]
        (for [i (range (count preds))]
          (update-in q [:where :preds] drop-nth i))))
    (when (> (count (get-in q [:return :items])) 1)
      (for [i (range (count (get-in q [:return :items])))]
        (update-in q [:return :items] drop-nth i)))
    (when (get-in q [:return :order]) [(update q :return dissoc :order)])
    (when (get-in q [:return :skip]) [(update q :return dissoc :skip)])
    (when (get-in q [:return :limit]) [(update q :return dissoc :limit)])
    (when (get-in q [:return :distinct?]) [(update q :return dissoc :distinct?)])
    (when (get-in q [:with :where]) [(update q :with dissoc :where)])
    (when (some :optional? (:matches q))
      [(update q :matches (fn [ms] (vec (remove :optional? ms))))])
    (when (> (count (get-in q [:matches 0 :groups])) 1)
      (for [i (range (count (get-in q [:matches 0 :groups])))]
        (update-in q [:matches 0 :groups] drop-nth i)))
    (map #(assoc q :matches %)
         (indexed-variants (:matches q) clause-candidates))
    (lit-variants q)))

(defn op-candidates
  "The candidate set for any op data, dispatched by kind."
  [data]
  (case (:kind data)
    :read (query-candidates data)
    (:set :remove :delete) (write-candidates data)
    (:create-node :create-rel) (setup-candidates data)
    []))
