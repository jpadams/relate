"""DB-API 2.0 type objects and constructors for Relate.

PEP 249 mandates a set of type-comparison singletons (``STRING``, ``BINARY``,
``NUMBER``, ``DATETIME``, ``ROWID``) used as ``Cursor.description`` type codes,
plus constructors (``Date``, ``Time``, ``Timestamp``, ``Binary``, and their
``*FromTicks`` variants).

Relate results are dynamically typed (Cypher has no fixed column schema), so
each result column's type is stamped by the engine from its first row
(ADR-0224) as one of 18 canonical names (``STRING``, ``INTEGER``, ``FLOAT``,
``BOOLEAN``, ``NODE``, ``RELATIONSHIP``, ``PATH``, ``LIST``, ``MAP``,
``POINT``, ``DATETIME``, ``DATE``, ``TIME``, ``DURATION``, ``VECTOR``,
``UUID``, ``BINARY``, ``ANY``). ``Cursor.description`` maps that engine name
onto the PEP 249 type-comparison objects via :func:`engine_type_code`.
"""

import datetime as _dt
import time as _time


class _DBAPITypeObject:
    """A DB-API type-comparison singleton.

    Equal to itself or to another ``_DBAPITypeObject`` with the same name,
    so ``description`` type codes can be compared with, e.g.,
    ``code == relate.NUMBER``.
    """

    def __init__(self, name):
        self.name = name

    def __eq__(self, other):
        return other is self or (
            isinstance(other, _DBAPITypeObject) and other.name == self.name
        )

    def __hash__(self):
        return hash(self.name)

    def __repr__(self):
        return f"<DBAPIType {self.name}>"


STRING = _DBAPITypeObject("STRING")
BINARY = _DBAPITypeObject("BINARY")
NUMBER = _DBAPITypeObject("NUMBER")
DATETIME = _DBAPITypeObject("DATETIME")
ROWID = _DBAPITypeObject("ROWID")

# Engine column-type name (ADR-0224) -> PEP 249 type-comparison object.
# INTEGER/FLOAT/BOOLEAN fold into NUMBER; DATE/TIME fold into DATETIME
# (temporal family); any other canonical name (NODE, RELATIONSHIP, PATH,
# LIST, MAP, POINT, DURATION, VECTOR, UUID, ANY) has no DB-API code and
# reports ``None`` -- including ``ANY``, the empty-result / Null stamp.
_ENGINE_TYPE_CODES = {
    "STRING": STRING,
    "BINARY": BINARY,
    "INTEGER": NUMBER,
    "FLOAT": NUMBER,
    "BOOLEAN": NUMBER,
    "DATETIME": DATETIME,
    "DATE": DATETIME,
    "TIME": DATETIME,
}


def engine_type_code(name):
    """The PEP 249 type code for an engine column-type name (ADR-0224), or
    ``None`` when the name has no DB-API counterpart (graph, spatial,
    temporal-duration and list/map types, and ``ANY``). ``None`` input — a
    column with no engine type stamp — also reports ``None``."""
    return _ENGINE_TYPE_CODES.get(name)


# --- DB-API constructors -------------------------------------------------

Date = _dt.date
Time = _dt.time
Timestamp = _dt.datetime
Binary = bytes


def DateFromTicks(ticks):
    return Date(*_time.localtime(ticks)[:3])


def TimeFromTicks(ticks):
    return Time(*_time.localtime(ticks)[3:6])


def TimestampFromTicks(ticks):
    return Timestamp(*_time.localtime(ticks)[:6])
