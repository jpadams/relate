(ns jepsen.relate.compare-test
  "Unit contracts for the compare lane's differential engine and the
  nemesis's wire-down evidence, without a docker roundtrip:

  - diff/canonical: every finite number keys through ONE path, so
    numerically-equal values (integral doubles and their integer form,
    at any magnitude) render identically in row-freqs;
  - shrink/weight: the string shrink is strictly decreasing;
  - diff/exec!: a connection-level Neo4jException propagates (an
    infrastructure fault), never an {:outcome :error};
  - nemesis wire-down?: only a connection-level probe failure is 'down';
    an identity mismatch propagates."
  (:require [clojure.test :refer [deftest is testing]]
            [jepsen.relate [client :as c]
                               [nemesis]]
            [jepsen.relate.compare [diff :as diff]
                                       [shrink :as shrink]])
  (:import (org.neo4j.driver Driver)
           (org.neo4j.driver.exceptions ClientException
                                       ServiceUnavailableException
                                       SessionExpiredException)))

(deftest canonical-unifies-numerically-equal-values
  (testing "integral doubles key with their integer form at any magnitude"
    (doseq [[d l] [[42.0 42] [0.0 0] [-0.0 0] [100.0 100]
                   [1.0E16 10000000000000000]
                   [-9.0E15 -9000000000000000]]]
      (is (= (pr-str (diff/canonical d)) (pr-str (diff/canonical l)))
          (str d " vs " l))))
  (testing "distinct values stay distinct"
    (is (not= (pr-str (diff/canonical 0.5)) (pr-str (diff/canonical 0))))
    (is (not= (pr-str (diff/canonical 9007199254740993))
              (pr-str (diff/canonical 9007199254740992)))))
  (testing "non-finite doubles keep their own tags"
    (is (= [:num :nan] (diff/canonical Double/NaN)))
    (is (= [:num :pos-inf] (diff/canonical Double/POSITIVE_INFINITY)))))

(deftest string-shrink-strictly-decreases-weight
  (is (= 1 (shrink/weight "a")))
  (doseq [s ["" "b" "abc"]]
    (is (< (shrink/weight "a") (shrink/weight s)) (pr-str s))))

(defn- driver-throwing
  "A Driver whose session open throws `t` — the exec! path's first wire
  touch."
  ^Driver [^Throwable t]
  (reify Driver
    (session [_] (throw t))))

(deftest exec-rethrows-connection-level-failures
  (doseq [t [(ServiceUnavailableException. "down")
             (SessionExpiredException. "expired")]]
    (is (thrown? Exception (diff/exec! (driver-throwing t) "RETURN 1" {}))
        (.getName (class t))))
  (testing "a genuine server FAILURE stays an outcome"
    (is (= {:outcome :error :msg "bad"}
           (diff/exec! (driver-throwing (ClientException. "bad"))
                       "RETURN 1" {})))))

(deftest wire-down-only-on-connection-failures
  (let [wire-down? @#'jepsen.relate.nemesis/wire-down?
        opts (fn [probe] {:uri "bolt://x" :auth-user "u" :auth-password "p"
                          :impl {:probe probe}})]
    (is (false? (wire-down? (opts (fn [& _] c/bolt-identity)))))
    (is (true? (wire-down? (opts (fn [& _] (throw (ServiceUnavailableException. "down")))))))
    (is (true? (wire-down? (opts (fn [& _] (throw (java.io.IOException. "refused")))))))
    (is (thrown-with-msg? clojure.lang.ExceptionInfo #"identity mismatch"
                          (wire-down? (opts (fn [& _] (c/assert-identity! "Neo4j/0.0.0"))))))))
