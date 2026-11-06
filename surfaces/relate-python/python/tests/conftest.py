"""Pytest fixtures for Relate Python package tests.

Requires the native extension to be built first, e.g.:

    cd relate-rs/surfaces/relate-python
    uvx maturin develop   # or: pip install -e .
"""

import pytest


@pytest.fixture
def db():
    """Provide an in-memory Relate connection, closed after the test."""
    import relate

    conn = relate.connect(":memory:")
    yield conn
    conn.close()


@pytest.fixture
def native_db():
    """Provide an in-memory native ``relate.Database``, closed after the test."""
    import relate

    db = relate.Database(":memory:")
    yield db
    db.close()
