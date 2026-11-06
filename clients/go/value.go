package relate

// The Go values a result cell decodes into (ADR-0502 §1). Each type is one
// engine value kind; the walker in walk.go builds them from the C ABI's
// components. Temporals are components, not ISO text: the zoned kinds carry
// their local wall clock plus the offset (and, for DATETIME, the IANA zone id
// when the engine kept one).

// Node is a NODE cell. ElementID is "" in the "system" namespace, which has no
// element ids. ID is the deprecated numeric entity id — exactly the INTEGER
// Cypher id() returns for the same entity (ADR-0506); prefer ElementID for
// lookups. Labels and Properties keep the engine's order.
type Node struct {
	ElementID  string
	ID         int64
	Labels     []string
	Properties map[string]any
}

// Relationship is a RELATIONSHIP cell. Type is its relationship type. ID is
// the deprecated numeric entity id (ADR-0506) — Cypher id(r) — and StartID /
// EndID the endpoints' numeric ids, id(startNode(r)) / id(endNode(r)); prefer
// the element ids for lookups.
type Relationship struct {
	ElementID      string
	StartElementID string
	EndElementID   string
	ID             int64
	StartID        int64
	EndID          int64
	Type           string
	Properties     map[string]any
}

// Path is a PATH cell: its elements alternate Node, Relationship, ..., Node
// (2n+1 elements for n hops).
type Path []any

// Date is a DATE cell.
type Date struct {
	Year, Month, Day int64
}

// LocalTime is a LOCAL_TIME cell. Nanosecond is 0-999999999.
type LocalTime struct {
	Hour, Minute, Second, Nanosecond int64
}

// Time is a TIME cell: a wall clock with its UTC offset. TIME keeps no zone id.
type Time struct {
	Hour, Minute, Second, Nanosecond int64
	OffsetSeconds                    int64
}

// LocalDateTime is a LOCAL_DATETIME cell.
type LocalDateTime struct {
	Year, Month, Day                 int64
	Hour, Minute, Second, Nanosecond int64
}

// DateTime is a DATETIME cell. Zone is the IANA zone id ("Europe/Stockholm"),
// or "" when the value carries only an offset.
type DateTime struct {
	Year, Month, Day                 int64
	Hour, Minute, Second, Nanosecond int64
	OffsetSeconds                    int64
	Zone                             string
}

// Duration is a DURATION cell as its four lossless components.
type Duration struct {
	Months, Days, Seconds, Nanos int64
}

// Point is a POINT cell. Z is written only when Dimension is 3.
type Point struct {
	SRID      int32
	Dimension int32
	X, Y, Z   float64
}

// Vector is a VECTOR cell. DType is the engine's canonical coordinate-type
// name. Ints holds the coordinates of the INTEGER* dtypes and Floats those of
// the FLOAT* dtypes; the other slice is nil.
type Vector struct {
	DType  string
	Ints   []int64
	Floats []float64
}
