(ns jepsen.relate.compare.render
  "The one canonical data->Cypher path for both engines (LABS-657): every
  graph-setup statement, read query, and write the compare workload
  generates (jepsen.relate.compare.gen) and every shrink candidate the
  differential engine re-executes (jepsen.relate.compare.diff) is
  plain Clojure data rendered to Cypher text HERE — generation,
  execution, and shrinking can never disagree on what a datum means.
  Also home to the canonical state-read statements mutations are
  compared through (diff/state-read)."
  (:require [clojure.string :as str]))

;;;; Rendering

(defn- qstr [s]
  (str \' (str/escape s {'\' "\\'"}) \'))

(defn- render-lit [v]
  (cond
    (nil? v)       "null"
    (string? v)    (qstr v)
    (keyword? v)   (name v)
    (sequential? v) (str "[" (str/join ", " (map render-lit v)) "]")
    (number? v)    (str v)
    (boolean? v)   (str v)
    :else (throw (ex-info (str "cannot render literal " (pr-str v))
                          {:lit v}))))

(defn- render-prop-value [v]
  (if (and (map? v) (= :param (:kind v)))
    (str "$" (name (:name v)))
    (render-lit v)))

(defn- render-props [props]
  (str "{"
       (str/join ", " (map (fn [[k v]] (str (name k) ": " (render-prop-value v)))
                           props))
       "}"))

(defn- render-node-pattern [{:keys [var labels props]}]
  (str "(" (some-> var name)
       (str/join (map #(str ":" %) labels))
       (when (seq props) (str " " (render-props props)))
       ")"))

(defn- render-range [[lo hi]]
  (cond (= lo hi) (str lo)
        (and lo hi) (str lo ".." hi)
        hi (str ".." hi)
        :else (str lo "..")))

(defn- render-rel-pattern [{:keys [var types range dir]}]
  (let [body (str (some-> var name)
                  (when (seq types) (str ":" (str/join "|" types)))
                  (when range (str "*" (render-range range))))
        [l rg] (case dir
                 (:right :none) ["-" "->"]
                 ["<-" "-"])]
    (str l "[" body "]" rg)))

(defn- render-chain
  "One path-pattern chain: alternating node/relationship patterns, e.g.
  (a:Person)-[r:KNOWS]->(b)."
  [chain]
  (apply str (map (fn [el]
                    (case (:type el)
                      :node (render-node-pattern el)
                      :rel  (render-rel-pattern el)
                      (throw (ex-info (str "unknown pattern element " (pr-str el))
                                      {:el el}))))
                  chain)))

(declare render-pred)

(defn- render-expr [e]
  (case (:kind e)
    :lit     (render-lit (:value e))
    :param   (str "$" (name (:name e)))
    :prop    (str (name (:var e)) "." (name (:key e)))
    :node    (name (:var e))
    :rel     (name (:var e))
    :fn      (str (name (:fn e)) "("
                  (str/join ", " (map render-expr (:args e))) ")")
    :agg     (if (and (= :count (:agg e)) (empty? (:args e)))
               (str "count(" (when (:distinct? e) "DISTINCT ") "*)")
               (str (name (:agg e)) "("
                    (when (:distinct? e) "DISTINCT ")
                    (str/join ", " (map render-expr (:args e))) ")"))
    :arith   (str (render-expr (:lhs e)) " " (name (:op e)) " "
                  (render-expr (:rhs e)))
    :case    (str "CASE"
                  (str/join (for [[p v] (:whens e)]
                              (str " WHEN " (render-pred p)
                                   " THEN " (render-lit v))))
                  (when (some? (:else e)) (str " ELSE " (render-lit (:else e))))
                  " END")
    :path-props (str "[x IN nodes(" (name (:var e)) ") | x." (name (:key e)) "]")
    (throw (ex-info (str "cannot render expression " (pr-str e)) {:expr e}))))

(defn- render-pred [p]
  (case (:kind p)
    :cmp      (str (render-expr (:lhs p)) " " (name (:op p)) " "
                   (render-expr (:rhs p)))
    :starts   (str (render-expr (:expr p)) " STARTS WITH " (render-lit (:arg p)))
    :ends     (str (render-expr (:expr p)) " ENDS WITH " (render-lit (:arg p)))
    :contains (str (render-expr (:expr p)) " CONTAINS " (render-lit (:arg p)))
    :in       (str (render-expr (:expr p)) " IN " (render-lit (:list p)))
    :null     (str (render-expr (:expr p)) " IS " (when (:negated? p) "NOT ") "NULL")
    :not      (str "NOT (" (render-pred (:pred p)) ")")
    (throw (ex-info (str "cannot render predicate " (pr-str p)) {:pred p}))))

(defn- render-where [{:keys [preds join]}]
  (str "WHERE " (str/join (str " " (name join) " ") (map render-pred preds))))

(defn- render-matches [matches]
  (str/join " "
            (map (fn [{:keys [optional? groups path]}]
                   (str (when optional? "OPTIONAL ")
                        "MATCH "
                        (when path (str (name path) " = "))
                        (str/join ", " (map render-chain groups))))
                 matches)))

(defn- render-items [items]
  (str/join ", " (map (fn [{:keys [expr alias]}]
                        (str (render-expr expr)
                             (when alias (str " AS " (name alias)))))
                      items)))

(defn- render-return [{:keys [items distinct? order skip limit]}]
  (str "RETURN "
       (when distinct? "DISTINCT ")
       (render-items items)
       (when (seq order)
         (str " ORDER BY "
              (str/join ", " (map #(str (render-expr (:expr %))
                                        (when (:desc? %) " DESC"))
                                  order))))
       (when (pos? (or skip 0)) (str " SKIP " skip))
       (when limit (str " LIMIT " limit))))

(defn- render-read
  "A full read query: optional UNWIND, MATCH clauses (any but the first
  may be OPTIONAL), WHERE, one WITH stage, RETURN."
  [q]
  (str/join " "
            (filter seq
                    [(when (:unwind q)
                       (str "UNWIND " (render-expr (:expr (:unwind q)))
                            " AS " (name (:var (:unwind q)))))
                     (when (:matches q) (render-matches (:matches q)))
                     (when (:where q) (render-where (:where q)))
                     (when (:with q)
                       (str "WITH " (render-items (:items (:with q)))
                            (when (:where (:with q))
                              (str " WHERE " (render-pred (:where (:with q)))))))
                     (render-return (:return q))])))

(defn- render-setup-stmt
  "A graph-setup statement. Nodes are created standalone; relationships
  bind their endpoints by nid (the user-visible identity, never an
  engine-internal id)."
  [s]
  (case (:kind s)
    :create-node (str "CREATE " (render-node-pattern s))
    :create-rel  (str "MATCH (a {nid: " (:from s) "}), (b {nid: " (:to s) "}) "
                      "CREATE (a)-[:" (:type s)
                      (when (seq (:props s)) (str " " (render-props (:props s))))
                      "]->(b)")
    (throw (ex-info (str "unknown setup statement " (pr-str s)) {:stmt s}))))

(defn- render-write
  "A mutation op, always addressed by nid. SET carries at least one item;
  REMOVE at least one; DELETE is DETACH four times out of five (a bare
  DELETE on a related node errors on both engines, which is a no-op for
  comparison, not a divergence)."
  [w]
  (case (:kind w)
    :set    (str "MATCH (n {nid: " (:nid w) "}) SET "
                 (str/join ", " (concat
                                  (map (fn [[k v]]
                                         (str "n." (name k) " = " (render-lit v)))
                                       (:sets w))
                                  (map #(str "n:" %) (:add-labels w)))))
    :remove (str "MATCH (n {nid: " (:nid w) "}) REMOVE "
                 (str/join ", " (concat
                                  (map #(str "n." (name %)) (:remove-props w))
                                  (map #(str "n:" %) (:remove-labels w)))))
    :delete (str "MATCH (n {nid: " (:nid w) "}) "
                 (if (:detach w) "DETACH DELETE n" "DELETE n"))
    (throw (ex-info (str "unknown write op " (pr-str w)) {:write w}))))

(defn render-stmt
  "The one canonical op-data -> statement renderer, used by generation,
  execution, and shrinking alike."
  [data]
  (case (:kind data)
    :read (render-read data)
    (:set :remove :delete) (render-write data)
    (:create-node :create-rel) (render-setup-stmt data)
    (throw (ex-info (str "unknown op data " (pr-str data)) {:data data}))))

;;;; The canonical state read — how mutations are compared

(def state-read-nodes
  "Every node keyed by its user-visible nid, deterministic order."
  (str "MATCH (n) RETURN n.nid AS nid, labels(n) AS ls, properties(n) AS ps "
       "ORDER BY n.nid"))

(def state-read-rels
  "Every relationship keyed by endpoint nids + rid, deterministic order."
  (str "MATCH (a)-[r]->(b) RETURN a.nid AS f, b.nid AS t, type(r) AS ty, "
       "properties(r) AS ps ORDER BY a.nid, b.nid, r.rid"))
