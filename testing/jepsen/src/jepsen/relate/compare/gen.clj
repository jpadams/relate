(ns jepsen.relate.compare.gen
  "Random graph and op generation for the compare workload (LABS-657), a
  test.check-style generator over the shared openCypher floor.

  Everything here is DATA first: a graph, a setup statement, a read query,
  a write — each is plain Clojure data rendered to Cypher text by
  jepsen.relate.compare.render (the one canonical data->Cypher path).
  The data is what the shrinker shrinks (jepsen.relate.compare.shrink
  enumerates the candidates, jepsen.relate.compare.diff adopts them):
  dropping a node property, a WHERE predicate, a RETURN item, or
  simplifying a literal re-renders to a strictly smaller statement (see
  shrink/weight), and shrinking is what turns a raw divergence into a
  minimal Cypher case.

  Two rules are load-bearing for 'zero false positives':

  - FLOOR ONLY. Every construct is in the openCypher TCK 2024.3 floor both
    engines implement: no Neo4j-only syntax, no Relate preview
    features. WHERE predicates compare same-typed operands only (Cypher
    errors on cross-type comparison, and an error on both engines is not
    a divergence), string predicates target string properties only, and
    `/` and `%` literals are nonzero.

  - NO ORDER-DEPENDENT FLOATS. Floats are exact binary fractions
    (0.5, 1.25, 2.5, 3.75) used as literals and per-row arithmetic only.
    No sum/avg over floats and no collect(): row order and summation
    order are unspecified by the language, so anything accumulating
    floats across rows in engine-chosen order would be a false divergence
    factory, not a bug finder.

  Node and relationship identity is user-visible only: every generated
  node carries a unique `nid` and every relationship a unique `rid`
  property, so canonical state comparison (diff/state-read) never touches
  engine-internal ids, which differ by construction."
  (:require [jepsen.relate.compare [render :as render]]))

;;;; Vocabulary — deliberately tiny, so duplicates and collisions are common

(def ^:private labels ["Person" "Company" "City"])
(def ^:private extra-label "Active")
(def ^:private names ["Alice" "Bob" "Carol" "Dave"])
(def ^:private rel-types ["KNOWS" "LIKES" "WORKS_FOR"])
(def ^:private scores [0.5 1.25 2.5 3.75])
(def ^:private node-prop-keys [:name :num :score :active])
(def ^:private string-prefixes ["A" "B" "li" "ro"])

;;;; RNG

(defn rng [seed] (java.util.Random. (long seed)))

(defn- rint
  "Inclusive integer in [lo, hi]."
  ^long [^java.util.Random r ^long lo ^long hi]
  (assert (<= lo hi) "rint requires lo <= hi")
  (+ lo (.nextLong r (inc (- hi lo)))))

(defn- rpick [r coll] (nth coll (rint r 0 (dec (count coll)))))

(defn- chance? [^java.util.Random r prob] (< (.nextDouble r) (double prob)))

;;;; Graph generation

(defn- gen-node
  "One node: unique `nid` and a `name` always; `num`, `score`, `active`
  each present two times out of three — absent properties are what make
  IS NULL / coalesce / null-propagation worth testing."
  [r nid]
  (let [primary (rpick r labels)]
    {:labels (if (chance? r 0.2) [primary extra-label] [primary])
     :props (cond-> (array-map :nid nid :name (rpick r names))
              (chance? r 2/3) (assoc :num (rint r 0 9))
              (chance? r 2/3) (assoc :score (rpick r scores))
              (chance? r 2/3) (assoc :active (chance? r 0.5)))}))

(defn- gen-rel
  "One relationship between random nids; 10% self-loops (legal Cypher —
  and exactly the kind of corner a differential lane should poke)."
  [r rid node-count]
  (let [from (rint r 1 node-count)
        to (if (chance? r 0.1) from (rint r 1 node-count))]
    {:type (rpick r rel-types)
     :from from
     :to to
     :props (cond-> (array-map :rid rid)
              (chance? r 2/3) (assoc :since (rint r 2000 2005)))}))

(defn gen-graph
  "A small graph: 3-8 nodes, node-count..node-count+4 relationships
  (capped at 12)."
  [r]
  (let [n (rint r 3 8)
        nodes (mapv #(gen-node r %) (range 1 (inc n)))
        nrels (min 12 (+ n (rint r 0 4)))
        rels (mapv #(gen-rel r % n) (range 1 (inc nrels)))]
    {:node-count n :nodes nodes :rels rels}))

(defn setup-stmts
  "The graph as an ordered list of setup statements: every node, then
  every relationship. Rel creation after all nodes exist, so its MATCH
  always finds both endpoints."
  [{:keys [nodes rels]}]
  (vec (concat
         (map (fn [node] (let [d (assoc node :kind :create-node)]
                           {:data d :stmt (render/render-stmt d)}))
              nodes)
         (map (fn [rel] (let [d (assoc rel :kind :create-rel)]
                          {:data d :stmt (render/render-stmt d)}))
              rels))))

;;;; Read-query generation

(defn- param-expr [name] {:kind :param :name name})

(defn- node-var [r] (rpick r [:a :b :c :d]))

(defn- gen-node-pattern
  "A node pattern with optional label and optional property constraint
  (by nid, by name — which may be a parameter — or none)."
  [r var node-count param]
  (let [constraint (rint r 0 2)]
    {:type :node
     :var var
     :labels (if (chance? r 0.7) [(rpick r labels)] [])
     :props (case constraint
              0 {:nid (rint r 1 node-count)}
              1 (if (and param (= :str (:kind param)) (chance? r 0.5))
                  {:name (param-expr (:name param))}
                  {:name (rpick r names)})
              2 {})}))

(defn- gen-rel-pattern
  "A relationship pattern: direction sometimes undirected, type two times
  out of three, no inline properties (the WHERE clause covers those)."
  [r range?]
  {:type :rel
   :var (if (chance? r 0.7) :r nil)
   :types (if (chance? r 2/3)
            [(rpick r rel-types)]
            [])
   :range (when range?
            (case (rint r 0 2)
              0 [1 3] 1 [1 2] 2 [2 2]))
   :dir (rpick r [:right :right :right :left :none])})

(defn- gen-lit-expr
  "A literal expression of the same Cypher type as `key`'s values."
  [r key]
  {:kind :lit
   :value (case key
            :name (rpick r names)
            :num (rint r 0 9)
            :nid (rint r 1 9)
            :score (rpick r scores)
            :active (chance? r 0.5)
            :since (rint r 2000 2005))})

(defn- typed-rhs
  "The right-hand side of a comparison on `key`: a same-typed literal, or
  the op's single parameter when its kind matches."
  [r key param]
  (let [pk ({:name :str :num :int :nid :int :score :int :since :int}
            key)]
    (if (and param (= pk (:kind param)) (chance? r 0.8))
      (param-expr (:name param))
      (gen-lit-expr r key))))

(def ^:private cmp-ops [:< :> :<= :>= := :<>])

(defn- gen-pred
  "One WHERE predicate over an in-scope property. Type discipline: the
  comparison operand type matches the property's type (Cypher errors on
  cross-type comparison — an error on both engines is not a divergence);
  string predicates only on string properties; IN lists hold values of
  the property's type."
  [r var key param]
  (case (rint r 0 5)
    ;; Two of the arms: comparisons carry double weight.
    (0 1) {:kind :cmp
           :op (rpick r cmp-ops)
           :lhs {:kind :prop :var var :key key}
           :rhs (typed-rhs r key param)}
    2 (if (= :name key)
        {:kind (rpick r [:starts :ends :contains])
         :expr {:kind :prop :var var :key key}
         :arg (rpick r string-prefixes)}
        {:kind :null :negated? (chance? r 0.5)
         :expr {:kind :prop :var var :key key}})
    3 {:kind :null :negated? (chance? r 0.5)
       :expr {:kind :prop :var var :key key}}
    4 (let [k (if (= :name key) :name (rpick r [:num :nid :score]))]
        {:kind :in
         :expr {:kind :prop :var var :key k}
         :list (case k
                 :name (vec (repeatedly (rint r 1 3) #(rpick r names)))
                 :score (vec (repeatedly (rint r 1 3) #(rpick r scores)))
                 (vec (repeatedly (rint r 1 4) #(rint r 0 9))))})
    5 {:kind :not
       :pred {:kind :cmp :op := :lhs {:kind :prop :var var :key key}
              :rhs (typed-rhs r key param)}}))

(defn- gen-where
  "1-3 predicates over the bound variables, joined uniformly (all AND or
  all OR — one precedence level, so rendering needs no extra parens)."
  [r vars param]
  (let [pairs (distinct (for [v (map :var vars)
                              k node-prop-keys]
                          [v k]))]
    {:preds (vec (distinct (repeatedly (rint r 1 3)
                                       #(let [[v k] (rpick r pairs)]
                                          (gen-pred r v k param)))))
     :join (if (chance? r 0.7) :and :or)}))

(defn- gen-scalar-item
  "One RETURN item over the bound variables. All expressions are scalar
  and comparable across engines: properties, labels()/type()/
  properties(), per-row functions, arithmetic, CASE. Never a bare node
  or relationship value (engine-internal ids would make comparison
  meaningless) and never a float accumulator."
  [r node-vars rel-vars alias]
  (let [menu
        (concat
          ;; properties
          (for [v node-vars k node-prop-keys]
            {:kind :prop :var v :key k})
          (for [v rel-vars] {:kind :prop :var v :key :since})
          ;; entity functions
          (for [v node-vars] {:kind :fn :fn :labels :args [{:kind :node :var v}]})
          (for [v node-vars] {:kind :fn :fn :properties
                              :args [{:kind :node :var v}]})
          (for [v rel-vars] {:kind :fn :fn :type :args [{:kind :rel :var v}]})
          ;; per-row scalar functions
          (for [v node-vars] {:kind :fn :fn :size
                              :args [{:kind :prop :var v :key :name}]})
          (for [v node-vars] {:kind :fn :fn :toString
                              :args [{:kind :prop :var v :key :num}]})
          (for [v node-vars] {:kind :fn :fn :coalesce
                              :args [{:kind :prop :var v :key :score}
                                     {:kind :lit :value (rpick r scores)}]})
          ;; per-row arithmetic (ints; float + exact fraction stays exact)
          (for [v node-vars]
            {:kind :arith :op (rpick r [:+ :- :*])
             :lhs {:kind :prop :var v :key :num}
             :rhs {:kind :lit :value (rint r 1 5)}})
          (for [v node-vars]
            {:kind :arith :op :+ :lhs {:kind :prop :var v :key :score}
             :rhs {:kind :lit :value 0.5}})
          ;; CASE
          (for [v node-vars]
            {:kind :case
             :whens [[{:kind :cmp :op :>
                       :lhs {:kind :prop :var v :key :num}
                       :rhs (gen-lit-expr r :num)}
                      "big"]]
             :else "small"}))]
    {:expr (rpick r (vec menu))
     :alias (when alias (rpick r [:v :w :x]))}))

(defn- gen-return
  "1-3 scalar items, sometimes DISTINCT, sometimes ORDER BY, sometimes
  SKIP/LIMIT. The ORDER BY sorts on one of the items' expressions.
  Shrinking (shrink.clj's query-candidates) drops items and the ORDER BY
  as independent candidates, so a shrunk query may order by an
  expression it no longer returns: legal Cypher without DISTINCT (the
  variables stay in scope), an error on both engines with it — and an
  error on both engines is not a divergence, so such a candidate never
  stands in for the reproducer."
  [r node-vars rel-vars]
  (let [items (vec (repeatedly (rint r 1 3)
                               #(gen-scalar-item r node-vars rel-vars
                                                 (chance? r 0.5))))]
    (cond-> {:items items}
      (chance? r 0.15) (assoc :distinct? true)
      (chance? r 0.3) (assoc :order
                             [{:expr (rpick r (mapv :expr items))
                               :desc? (chance? r 0.3)}])
      (chance? r 0.2) (assoc :skip (rint r 0 2))
      (chance? r 0.2) (assoc :limit (rint r 1 3)))))

(def ^:private agg-names [:count :sum :min :max :avg])

(defn- gen-agg-item
  "One aggregate item: count(*) or an aggregate over an integer property
  (sum/avg never over floats — see the ns docstring)."
  [r node-vars distinct?]
  (if (chance? r 0.3)
    {:expr {:kind :agg :agg :count :distinct? distinct? :args []}
     :alias :c}
    (let [k (rpick r [:num :nid])]
      {:expr {:kind :agg :agg (rpick r agg-names) :distinct? distinct?
              :args [{:kind :prop :var (rpick r (vec node-vars)) :key k}]}
       :alias :c})))

;;;; The eight read templates

(defn- basic-read
  "T1/T2: a single MATCH — bare node pattern or a chain — WHERE, RETURN."
  [r n param kind]
  (let [v1 (node-var r)
        v2 (node-var r)
        rel (gen-rel-pattern r false)
        matches (if (= :node kind)
                  [{:optional? false
                    :groups [[(gen-node-pattern r v1 n param)]]}]
                  [{:optional? false
                    :groups [[(gen-node-pattern r v1 n param)
                              rel
                              (gen-node-pattern r v2 n nil)]]}])
        node-vars (if (= :node kind) [v1] (distinct [v1 v2]))
        rel-vars (if (:var rel) [(:var rel)] [])
        ;; RNG order: the WHERE draw precedes the RETURN draw, so a
        ;; seed reproduces the same query.
        where (when (chance? r 0.7)
                (gen-where r (map (fn [v] {:var v}) node-vars) param))
        ret (gen-return r node-vars rel-vars)]
    (assoc (cond-> {:kind :read :matches matches}
             where (assoc :where where))
           :return ret)))

(defn- path-read
  "T3: a named path over a variable-length relationship. Returns are
  path scalars only (length, node property sequences) — never the path
  value itself."
  [r n]
  (let [v1 (node-var r) v2 (node-var r)
        rel (-> (gen-rel-pattern r true)
                (assoc :dir :right :types [(rpick r rel-types)]))]
    {:kind :read
     :matches [{:optional? false :path :p
                :groups [[(gen-node-pattern r v1 n nil)
                          rel
                          (gen-node-pattern r v2 n nil)]]}]
     :return {:items (vec (concat [{:expr {:kind :fn :fn :length
                                            :args [{:kind :node :var :p}]}
                                   :alias :hops}]
                                  (for [k [:name :nid]]
                                    {:expr {:kind :path-props :var :p :key k}
                                     :alias (keyword (str "path" (name k)))})))}}))

(defn- optional-read
  "T4: a guaranteed MATCH plus an OPTIONAL MATCH chain — null rows for
  unmatched patterns are half of what this template exercises."
  [r n param]
  (let [v1 (node-var r) v2 (node-var r)
        rel (gen-rel-pattern r false)
        rel-vars (if (:var rel) [(:var rel)] [])]
    {:kind :read
     :matches [{:optional? false
                :groups [[(gen-node-pattern r v1 n param)]]}
               {:optional? true
                :groups [[(gen-node-pattern r v1 n nil)
                          rel
                          (gen-node-pattern r v2 n nil)]]}]
     :where (when (chance? r 0.4)
              (gen-where r [{:var v1} {:var v2}] param))
     :return (gen-return r (distinct [v1 v2]) rel-vars)}))

(defn- aggregate-read
  "T5: implicit grouping — one grouping item plus one aggregate. ORDER BY
  on the aggregate alias only (ordering by a grouping key with duplicate
  keys leaves tie order unspecified)."
  [r n]
  (let [v (node-var r)
        group {:expr {:kind :prop :var v :key (rpick r [:name :num])}
               :alias :g}]
    {:kind :read
     :matches [{:optional? false
                :groups [[(gen-node-pattern r v n nil)]]}]
     :where (when (chance? r 0.3)
              (gen-where r [{:var v}] nil))
     :return (cond-> {:items [group (gen-agg-item r [v] false)]}
               (chance? r 0.4) (assoc :order [{:expr {:kind :prop :var v
                                                      :key (rpick r [:name :num])}
                                              :desc? (chance? r 0.5)}])
               (chance? r 0.2) (assoc :limit (rint r 1 3)))}))

(defn- with-read
  "T6: aggregate in a WITH stage, filtered, then returned."
  [r n]
  (let [v (node-var r)]
    {:kind :read
     :matches [{:optional? false
                :groups [[(gen-node-pattern r v n nil)]]}]
     :with {:items [{:expr {:kind :prop :var v :key (rpick r [:name :num])}
                     :alias :g}
                    (gen-agg-item r [v] true)]
            :where {:kind :cmp :op (rpick r [:> :>=])
                    :lhs {:kind :node :var :c}
                    :rhs {:kind :lit :value (rint r 0 2)}}}
     :return {:items [{:expr {:kind :node :var :g} :alias :g2}
                      {:expr {:kind :node :var :c} :alias :c2}]}}))

(defn- two-pattern-read
  "T7: comma-separated patterns — a cartesian match."
  [r n param]
  (let [v1 (node-var r) v2 (node-var r)]
    {:kind :read
     :matches [{:optional? false
                :groups [[(gen-node-pattern r v1 n param)]
                         [(gen-node-pattern r v2 n nil)]]}]
     :where (when (chance? r 0.6)
              (gen-where r [{:var v1} {:var v2}] param))
     :return (gen-return r (distinct [v1 v2]) [])}))

(defn- unwind-read
  "T8: UNWIND a literal or parameter list, then match by the unwound
  value."
  [r n param]
  (let [v (node-var r)
        use-param (and param (= :list (:kind param)))
        {:keys [value kind]} (if use-param
                               {:value (:value param) :kind :param}
                               {:value (vec (repeatedly (rint r 1 4)
                                                        #(rpick r names)))
                                :kind :lit})
        expr (if (= :param kind)
               (param-expr (:name param))
               {:kind :lit :value value})]
    {:kind :read
     :unwind {:var :x :expr expr}
     :matches [{:optional? false
                :groups [[(gen-node-pattern r v n nil)]]}]
     :where {:preds [{:kind :cmp :op :=
                      :lhs {:kind :prop :var v :key :name}
                      :rhs {:kind :node :var :x}}]
             :join :and}
     :return (gen-return r [v] [])}))

;;;; Write generation

(defn- gen-write
  "A mutation addressed by nid: SET (property values, sometimes a label),
  REMOVE (properties, sometimes a label), or DELETE (DETACH four times
  out of five)."
  [r node-count]
  (let [nid (rint r 1 node-count)]
    (case (rint r 0 9)
      (0 1 2 3 4 5) {:kind :set :nid nid
                     :sets (cond-> (if (chance? r 0.5)
                                     (array-map :num (rint r 0 9))
                                     (array-map :name (rpick r names)))
                              (chance? r 0.4) (assoc :score (rpick r scores))
                              (chance? r 0.3) (assoc :active (chance? r 0.5)))
                     :add-labels (when (chance? r 0.25) [extra-label])}
      (6 7 8)       {:kind :remove :nid nid
                     :remove-props (rpick r [[:num] [:score] [:num :active]])
                     :remove-labels (when (chance? r 0.4) [extra-label])}
      9             {:kind :delete :nid nid :detach (chance? r 0.8)})))

;;;; Read dispatcher and the op plan

(defn- gen-param
  "The op's single parameter slot, or nil. :kind chooses which literal
  sites can take it: :int for numeric comparisons, :str for name
  equality/property-map constraints, :list for the UNWIND source."
  [r]
  (when (chance? r 1/4)
    (case (rint r 0 2)
      0 {:kind :int :name :p0 :value (rint r 0 9)}
      1 {:kind :str :name :p0 :value (rpick r names)}
      {:kind :list :name :p0
       :value (vec (repeatedly (rint r 1 4) #(rpick r names)))})))

(defn- param-slots
  "The parameter slots query data `q` consumes: the :name of every
  {:kind :param} expression in the tree — the typed fact the templates'
  own param wiring leaves behind (gen-node-pattern's name constraint,
  typed-rhs, unwind-read's source), read structurally, never from the
  rendered text."
  [q]
  (into #{}
        (comp (filter #(and (map? %) (= :param (:kind %))))
              (map :name))
        (tree-seq coll? seq q)))

(defn- gen-read-op
  "One read op: pick a template, build its data, record the consumed
  parameter slots on it (:param-slots), render it once. The params map
  carries the op's parameter only when a template consumed its slot —
  passing an unconsumed param would be silently ignored by both
  engines, so it is dropped instead."
  [r node-count]
  (let [param (gen-param r)
        q (case (rint r 0 19)
            (0 1 2 3 4) (basic-read r node-count param (rpick r [:node :rel]))
            (5 6)       (basic-read r node-count param :rel)
            (7 8)       (path-read r node-count)
            (9 10)      (optional-read r node-count param)
            (11 12 13)  (aggregate-read r node-count)
            (14 15)     (with-read r node-count)
            (16 17)     (two-pattern-read r node-count param)
            (18 19)     (unwind-read r node-count param))
        q (assoc q :param-slots (param-slots q))]
    {:kind :read :data q :stmt (render/render-stmt q)
     :params (when (and param (contains? (:param-slots q) (:name param)))
               {(name (:name param)) (:value param)})}))

(defn gen-ops
  "The phase-2 op plan: `n` ops, ~30% writes, reads otherwise. Writes and
  reads interleave, so reads observe mutated state."
  [r node-count n]
  (vec (repeatedly n #(if (chance? r 0.3)
                        (let [w (gen-write r node-count)]
                          {:kind :write :data w :stmt (render/render-stmt w)
                           :params {}})
                        (gen-read-op r node-count)))))
