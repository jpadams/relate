# Geospatial queries

Relate's Cypher dialect has one geospatial type — the point — with a
constructor, a distance function, a bounding-box predicate and a point index.
Geometry work (routes, regions) happens in the host language: store the
geometry (a GeoJSON string, say) as an ordinary property and compute with a
geometry library there.

## Create points

### Geographic points

Use `latitude` and `longitude` keys (optionally `height` for 3D, in meters):

```cypher
CREATE (:City {
  name: 'New York',
  location: point({latitude: 40.7128, longitude: -74.0060})
});
```

### Cartesian points

Use `x` and `y` keys (optionally `z` for 3D):

```cypher
CREATE (:Sensor {
  id: 'S1',
  position: point({x: 10.0, y: 20.0})
});
```

## Distance

`point.distance(p1, p2)` returns the distance between two points. For geographic
points (latitude/longitude) it uses the Haversine formula with an earth radius of
6,378,140 m (matching Neo4j) and returns **meters**.
For cartesian points it uses Euclidean distance.

```cypher
MATCH (a:City {name: 'New York'}), (b:City {name: 'Los Angeles'})
RETURN point.distance(a.location, b.location) / 1000 AS distanceKm;
```

Find nearest locations:

```cypher
MATCH (c:City)
WITH c, point.distance(c.location, point({latitude: 40.7128, longitude: -74.0060})) AS dist
RETURN c.name, dist / 1000 AS distanceKm
ORDER BY dist
LIMIT 3;
```

## Bounding box

`point.withinBBox(pt, lowerLeft, upperRight)` returns true if `pt` falls within
the rectangular region defined by the lower-left and upper-right corners:

```cypher
MATCH (c:City)
WHERE point.withinBBox(c.location,
  point({latitude: 30.0, longitude: -80.0}),
  point({latitude: 45.0, longitude: -70.0}))
RETURN c.name;
```

## Radius check

A radius filter is a distance comparison — `point.distance()` from the center,
compared to the radius in meters:

```cypher
MATCH (c:City)
WHERE point.distance(c.location,
  point({latitude: 40.7128, longitude: -74.0060})) < 500000
RETURN c.name;
```

The `500000` bound is meters (500 km). Keep the distance call on the left of
the comparison: `point.distance(prop, center) < bound` is the shape the
point-index seek recognizes.

## Reading coordinates

A point's coordinates are plain properties, so they cross into the host
language without a conversion function: `p.latitude`/`p.longitude` for a
geographic point, `p.x`/`p.y`/`p.z` for a cartesian one.

```cypher
MATCH (c:City {name: 'Seattle'})
RETURN c.location.latitude AS lat, c.location.longitude AS lon;
```

## Point index

Create a point index to accelerate bounding-box and radius queries with an
R-tree:

```cypher
CREATE POINT INDEX city_locations FOR (c:City) ON (c.location);
```

With the index in place, a `WHERE` predicate of the shape
`point.withinBBox(c.location, lo, hi)` or
`point.distance(c.location, center) < bound` narrows the candidate nodes
through the R-tree before the exact coordinate check runs. The R-tree is
rebuilt from stored index metadata when the database is reopened.
