# Geospatial Queries

Relate supports geospatial data through the `point()` type and `point.distance()` function.

## Create geographic points

Use `latitude` and `longitude` for geographic coordinates:

```cypher
CREATE (:City {name: 'New York', location: point({latitude: 40.7128, longitude: -74.0060})});
CREATE (:City {name: 'Los Angeles', location: point({latitude: 34.0522, longitude: -118.2437})});
CREATE (:City {name: 'Chicago', location: point({latitude: 41.8781, longitude: -87.6298})});
CREATE (:City {name: 'London', location: point({latitude: 51.5074, longitude: -0.1278})});
```

## Calculate distance between points

`point.distance()` uses the Haversine formula for geographic coordinates with an earth radius of 6,378,140 m (matching Neo4j), returning distance in **meters**:

```cypher
MATCH (a:City {name: 'New York'}), (b:City {name: 'Los Angeles'})
RETURN point.distance(a.location, b.location) / 1000 AS distanceKm;
```

## Find nearest locations

```cypher
MATCH (c:City)
WITH c, point.distance(c.location, point({latitude: 40.7128, longitude: -74.0060})) AS dist
RETURN c.name, dist / 1000 AS distanceKm
ORDER BY dist
LIMIT 3;
```

## Radius search

Find all cities within 1000 km of a point:

```cypher
MATCH (c:City)
WITH c, point.distance(c.location, point({latitude: 40.7128, longitude: -74.0060})) AS dist
WHERE dist < 1000000
RETURN c.name, dist / 1000 AS distanceKm
ORDER BY dist;
```

(1000 km = 1,000,000 meters)

## Cartesian points

For 2D cartesian coordinates (e.g., floor plans, game maps), use `x` and `y`:

```cypher
CREATE (:Sensor {id: 'S1', position: point({x: 10.0, y: 20.0})});
CREATE (:Sensor {id: 'S2', position: point({x: 15.0, y: 25.0})});

MATCH (a:Sensor {id: 'S1'}), (b:Sensor {id: 'S2'})
RETURN point.distance(a.position, b.position) AS distance;
```

Cartesian distance uses the standard Euclidean formula.

## Combine with graph traversal

Find the nearest connected city via a route:

```cypher
MATCH (start:City {name: 'New York'})-[:ROUTE]-(connected:City)
WITH connected, point.distance(start.location, connected.location) AS dist
RETURN connected.name, dist / 1000 AS distanceKm
ORDER BY dist;
```

## 3D points

Geographic points support a `height` property (meters above sea level):

```cypher
CREATE (:Airport {
  name: 'Denver International',
  location: point({latitude: 39.8561, longitude: -104.6737, height: 1655})
});
```

Cartesian points support a `z` coordinate:

```cypher
CREATE (:Sensor {id: 'S3', position: point({x: 10.0, y: 20.0, z: 5.0})});
```

3D distance calculations account for the additional dimension.

## Bounding box and radius predicates

### Bounding box

Check if a point falls within a rectangular region:

```cypher
MATCH (c:City)
WHERE point.withinBBox(c.location,
  point({latitude: 30.0, longitude: -80.0}),
  point({latitude: 45.0, longitude: -70.0}))
RETURN c.name;
```

`point.withinBBox(pt, lowerLeft, upperRight)` compares the point's coordinates
against the two corners. For geographic points the corners are
latitude/longitude pairs, so the box is a lat/lon rectangle, not a great-circle
region.

### Radius check

A radius filter is a distance comparison — `point.distance()` from the center,
compared to the radius in meters:

```cypher
MATCH (c:City)
WHERE point.distance(c.location,
  point({latitude: 40.7128, longitude: -74.0060})) < 500000
RETURN c.name;
```

The `500000` bound is meters (500 km in this example). Keep the distance call
on the left of the comparison: `point.distance(prop, center) < bound` is the
shape the point-index seek recognizes.

## Geometry work in the host language

The Cypher dialect has points only — no geometry-type constructors,
topological predicates, or WKT/GeoJSON converters. Store a region's geometry as
an ordinary property (a GeoJSON string, for example) and do the geometry work
in the host language, where a full geometry library is available:

```python
import json
from shapely.geometry import shape

row = db.execute(
    "MATCH (b:Basin {name: $name}) RETURN b.geojson AS geojson",
    {"name": "Pacific Northwest"},
).fetchone()
region = shape(json.loads(row["geojson"]))
```

Point values cross the boundary as coordinates:
`p.latitude`/`p.longitude` (or `p.x`/`p.y`) are plain properties.

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
