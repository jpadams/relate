"""The engine's error classification on every exception (ADR-0502 §2):
the four ``ErrorInfo`` facts ride the native error and the mapped DB-API
exception verbatim — never re-derived, never fabricated."""

import threading

import pytest

import relate
from relate._native import RelateNativeError
from relate.exceptions import ENGINE_ERROR_ATTRS, NATIVE_ERROR_ATTRS, Error


def test_native_error_carries_engine_classification(native_db):
    with pytest.raises(RelateNativeError) as info:
        native_db.execute("RETURN $missing AS m")
    err = info.value
    assert err.neo4j_code == "Neo.ClientError.Statement.ParameterMissing"
    assert err.gql_status == "42N02"
    assert err.stage == "compile"
    assert err.retryable is False


def test_mapped_exception_carries_the_same_facts(db, native_db):
    with pytest.raises(RelateNativeError) as native:
        native_db.execute("RETURN 1 +")
    with pytest.raises(relate.ProgrammingError) as info:
        db.execute("RETURN 1 +")
    err = info.value
    assert err.neo4j_code == "Neo.ClientError.Statement.SyntaxError"
    assert err.stage == "compile"
    assert err.retryable is False
    # Copied verbatim from the native error, never re-derived — over the
    # engine-fact sub-vocabulary, so a new fact joins this check with the
    # mapping, not by a second hand-listed tuple.
    for name in ENGINE_ERROR_ATTRS:
        assert getattr(err, name) == getattr(native.value, name)
    # The binding vocabulary rides alongside.
    assert err.kind == "SyntaxError"
    assert err.tck_class == "SyntaxError"


def test_runtime_error_stage(db):
    with pytest.raises(relate.Error) as info:
        db.execute("UNWIND [1, 0] AS x RETURN 1 / x AS y")
    err = info.value
    assert err.stage == "runtime"
    assert err.neo4j_code.startswith("Neo.ClientError.")
    assert isinstance(err.gql_status, str) and len(err.gql_status) == 5
    assert err.retryable is False


def test_binding_level_failure_has_no_engine_facts(native_db):
    # A foreign-thread access is refused by the binding; the engine never
    # classified it, so the four facts are None, never fabricated.
    caught = []

    def worker():
        try:
            native_db.execute("RETURN 1")
        except RelateNativeError as e:  # pragma: no branch
            caught.append(e)

    t = threading.Thread(target=worker)
    t.start()
    t.join()
    (err,) = caught
    assert err.kind == "WrongThread"
    for name in ENGINE_ERROR_ATTRS:
        assert getattr(err, name) is None


def test_python_raised_error_has_no_engine_facts(db):
    db.close()
    with pytest.raises(relate.ProgrammingError) as info:
        db.execute("RETURN 1")
    assert info.value.neo4j_code is None and info.value.retryable is None


def test_error_fact_vocabulary_is_one_source():
    """The six error-fact names live in ``NATIVE_ERROR_ATTRS`` only
    (LABS-944): ``Error``'s class defaults are derived from that tuple, and
    connection.py's native-error mapping reads the same tuple — there is no
    second list to drift."""
    assert set(NATIVE_ERROR_ATTRS) == {
        "kind",
        "tck_class",
        "neo4j_code",
        "gql_status",
        "stage",
        "retryable",
    }
    for name in NATIVE_ERROR_ATTRS:
        assert getattr(Error, name) is None
