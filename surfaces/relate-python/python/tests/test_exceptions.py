"""Unit tests for Relate exception hierarchy."""

import pytest

from relate.exceptions import (
    RelateError,
    InterfaceError,
    OperationalError,
    ProgrammingError,
)


# --- Inheritance ---


def test_interface_error_is_relate_error():
    assert issubclass(InterfaceError, RelateError)


def test_operational_error_is_relate_error():
    assert issubclass(OperationalError, RelateError)


def test_programming_error_is_relate_error():
    assert issubclass(ProgrammingError, RelateError)


def test_all_are_exceptions():
    assert issubclass(RelateError, Exception)
    assert issubclass(InterfaceError, Exception)
    assert issubclass(OperationalError, Exception)
    assert issubclass(ProgrammingError, Exception)


# --- Catching base class catches all subclasses ---


def test_catch_base_catches_operational():
    with pytest.raises(RelateError):
        raise OperationalError("query failed")


def test_catch_base_catches_interface():
    with pytest.raises(RelateError):
        raise InterfaceError("pipe broken")


def test_catch_base_catches_programming():
    with pytest.raises(RelateError):
        raise ProgrammingError("closed connection")


# --- Message preservation ---


def test_operational_error_message():
    err = OperationalError("syntax error near 'FOO'")
    assert str(err) == "syntax error near 'FOO'"


def test_interface_error_message():
    err = InterfaceError("subprocess crashed")
    assert str(err) == "subprocess crashed"


def test_programming_error_message():
    err = ProgrammingError("Cannot operate on a closed connection")
    assert str(err) == "Cannot operate on a closed connection"


def test_empty_message():
    err = RelateError()
    assert str(err) == ""


# --- isinstance checks ---


def test_isinstance_hierarchy():
    err = OperationalError("test")
    assert isinstance(err, OperationalError)
    assert isinstance(err, RelateError)
    assert isinstance(err, Exception)
    assert not isinstance(err, InterfaceError)
    assert not isinstance(err, ProgrammingError)
