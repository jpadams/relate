"""Driver-style result rows (ADR-0213).

A ``Record`` is a ``dict`` — every mapping operation keeps working — plus the
Neo4j driver conveniences: attribute access, ``.data()``, and ``.keys()`` in
column order.
"""


class Record(dict):
    """One result row: a ``dict`` with Neo4j-driver-style conveniences.

    ``r["name"]`` (DB-API habit), ``r.name`` (attribute access), and
    ``r.data()`` (a plain ``dict`` copy) all read the same values. Column
    order is the statement's RETURN order, not insertion order. Rows are
    read-only through attributes: ``r.name = x`` raises ``AttributeError``
    (``r["name"] = x`` still works — a ``Record`` is a ``dict``).
    """

    __slots__ = ("_columns",)

    def __init__(self, columns=None, **values):
        super().__init__(values)
        self._columns = list(columns) if columns is not None else list(values)

    def __getattr__(self, name):
        try:
            return self[name]
        except KeyError:
            raise AttributeError(name) from None

    def keys(self):
        """Column names in the statement's RETURN order (a copy)."""
        return list(self._columns)

    def data(self):
        """A plain ``dict`` copy of the row."""
        return dict(self)

    def __reduce__(self):
        # Pickle round-trips through the column order plus the values.
        return (Record._wrap, (self._columns, dict(self)))

    @classmethod
    def _wrap(cls, columns, row):
        """Wrap one engine row (a plain dict) without changing its values."""
        rec = cls(columns)
        dict.update(rec, row)
        return rec
