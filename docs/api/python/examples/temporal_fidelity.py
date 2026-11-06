"""Temporal fidelity: results keep nanoseconds and IANA zone names.

A temporal result arrives as its stdlib type when that type carries the value
losslessly, and as ``relate.Temporal`` when it cannot: stdlib temporals hold
whole microseconds, and a stdlib ``datetime`` holds a fixed UTC offset rather
than a zone name.
"""

import relate

conn = relate.connect(":memory:")

# Microsecond-aligned, offset only: lossless as a stdlib datetime.
plain = conn.execute("RETURN datetime('2015-06-01T12:34:56.123456Z') AS v").single()["v"]
assert type(plain).__name__ == "datetime"
assert plain.microsecond == 123456

row = conn.execute(
    "RETURN datetime('2015-06-01T12:34:56.123456789Z') AS instant,"
    "       datetime('2020-10-25T02:30+02:00[Europe/Stockholm]') AS zoned,"
    "       time('12:31:14.645876123Z') AS tod"
).single()

# Sub-microsecond nanoseconds: the ninth digit stdlib cannot hold.
instant = row["instant"]
print(instant.text)          # 2015-06-01T12:34:56.123456789Z
print(instant.nanosecond)    # 123456789
print(instant.to_native())   # 2015-06-01 12:34:56.123456+00:00  (lossy)

# A named zone: the name survives, with the offset it implies at that instant.
zoned = row["zoned"]
print(zoned.text)            # 2020-10-25T02:30+02:00[Europe/Stockholm]
print(zoned.zone_name)       # Europe/Stockholm
print(zoned.offset_seconds)  # 7200 (CEST; the same zone reads 3600 in winter)

# A time of day keeps its nanoseconds too.
print(row["tod"].kind, row["tod"].nanosecond)  # time 645876123

# Equality is the engine's: same instant and zone. repr() shows the canonical form.
same = conn.execute(
    "RETURN datetime('2020-10-25T02:30+02:00[Europe/Stockholm]') AS v"
).single()["v"]
assert same == zoned
assert repr(zoned) == (
    "Temporal(kind=\"datetime\", text=\"2020-10-25T02:30+02:00[Europe/Stockholm]\")"
)

conn.close()
