//! GeoArrow encoding: CypherValue geometry <-> WKB binary.
//!
//! Provides encoding of Point, Geometry values to Well-Known Binary (WKB) format
//! for storage in Arrow Binary columns, and decoding back.
//! WKB is the standard encoding for GeoArrow extension arrays.

use arrow::error::ArrowError;
use arrow_array::{Array, ArrayRef, BinaryArray};
use relate_model::value::{Coord2D, PointValue};
use relate_model::{CypherValue, GeometryValue};
use std::sync::Arc;

/// WKB geometry type constants (little-endian).
const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTIPOINT: u32 = 4;
const WKB_MULTILINESTRING: u32 = 5;
const WKB_MULTIPOLYGON: u32 = 6;
const WKB_GEOMETRYCOLLECTION: u32 = 7;
pub(crate) const MAX_GEOMETRY_DEPTH: usize = 64;

fn write_coord(buf: &mut Vec<u8>, c: &Coord2D) {
    buf.extend_from_slice(&c[0].to_le_bytes());
    buf.extend_from_slice(&c[1].to_le_bytes());
}

/// Encode a PointValue to WKB bytes (little-endian, no SRID).
pub fn point_to_wkb(p: &PointValue) -> Vec<u8> {
    let mut buf = Vec::with_capacity(21);
    buf.push(1); // little-endian
    buf.extend_from_slice(&WKB_POINT.to_le_bytes());
    buf.extend_from_slice(&p.x.to_le_bytes());
    buf.extend_from_slice(&p.y.to_le_bytes());
    buf
}

/// Encode a GeometryValue to WKB bytes (little-endian).
pub fn geometry_to_wkb(geom: &GeometryValue) -> Vec<u8> {
    match geom {
        GeometryValue::LineString { coordinates, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_LINESTRING.to_le_bytes());
            buf.extend_from_slice(&(coordinates.len() as u32).to_le_bytes());
            for c in coordinates {
                write_coord(&mut buf, c);
            }
            buf
        }
        GeometryValue::Polygon { rings, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_POLYGON.to_le_bytes());
            buf.extend_from_slice(&(rings.len() as u32).to_le_bytes());
            for ring in rings {
                buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                for c in ring {
                    write_coord(&mut buf, c);
                }
            }
            buf
        }
        GeometryValue::MultiPoint { points, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_MULTIPOINT.to_le_bytes());
            buf.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for p in points {
                buf.push(1);
                buf.extend_from_slice(&WKB_POINT.to_le_bytes());
                write_coord(&mut buf, p);
            }
            buf
        }
        GeometryValue::MultiLineString { lines, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_MULTILINESTRING.to_le_bytes());
            buf.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            for line in lines {
                buf.push(1);
                buf.extend_from_slice(&WKB_LINESTRING.to_le_bytes());
                buf.extend_from_slice(&(line.len() as u32).to_le_bytes());
                for c in line {
                    write_coord(&mut buf, c);
                }
            }
            buf
        }
        GeometryValue::MultiPolygon { polygons, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_MULTIPOLYGON.to_le_bytes());
            buf.extend_from_slice(&(polygons.len() as u32).to_le_bytes());
            for polygon in polygons {
                buf.push(1);
                buf.extend_from_slice(&WKB_POLYGON.to_le_bytes());
                buf.extend_from_slice(&(polygon.len() as u32).to_le_bytes());
                for ring in polygon {
                    buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                    for c in ring {
                        write_coord(&mut buf, c);
                    }
                }
            }
            buf
        }
        GeometryValue::GeometryCollection { geometries, .. } => {
            let mut buf = Vec::new();
            buf.push(1);
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION.to_le_bytes());
            buf.extend_from_slice(&(geometries.len() as u32).to_le_bytes());
            for g in geometries {
                buf.extend_from_slice(&geometry_to_wkb(g));
            }
            buf
        }
    }
}

/// Encode a CypherValue (Point or Geometry) to WKB bytes.
/// Returns None if the value is not a spatial type.
pub fn cypher_value_to_wkb(val: &CypherValue) -> Option<Vec<u8>> {
    match val {
        CypherValue::Point(p) => Some(point_to_wkb(p)),
        CypherValue::Geometry(g) => Some(geometry_to_wkb(g)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Big,
    Little,
}

struct WkbReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> WkbReader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], ArrowError> {
        let end = self
            .offset
            .checked_add(size)
            .ok_or_else(|| invalid_wkb("byte offset overflow"))?;
        let bytes = self
            .data
            .get(self.offset..end)
            .ok_or_else(|| invalid_wkb("truncated geometry"))?;
        self.offset = end;
        Ok(bytes)
    }

    fn u32(&mut self, order: ByteOrder) -> Result<u32, ArrowError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| invalid_wkb("truncated u32"))?;
        Ok(match order {
            ByteOrder::Big => u32::from_be_bytes(bytes),
            ByteOrder::Little => u32::from_le_bytes(bytes),
        })
    }

    fn f64(&mut self, order: ByteOrder) -> Result<f64, ArrowError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| invalid_wkb("truncated f64"))?;
        Ok(match order {
            ByteOrder::Big => f64::from_be_bytes(bytes),
            ByteOrder::Little => f64::from_le_bytes(bytes),
        })
    }

    /// Verify a minimum footprint before starting any count-driven loop or allocation.
    fn count(&mut self, order: ByteOrder, minimum_bytes: usize) -> Result<usize, ArrowError> {
        let n = usize::try_from(self.u32(order)?)
            .map_err(|_| invalid_wkb("count does not fit in usize"))?;
        let end = n
            .checked_mul(minimum_bytes)
            .and_then(|bytes| self.offset.checked_add(bytes))
            .ok_or_else(|| invalid_wkb("count/offset overflow"))?;
        if end > self.data.len() {
            return Err(invalid_wkb("count exceeds remaining bytes"));
        }
        Ok(n)
    }

    fn header(
        &mut self,
        depth: usize,
        expected: Option<u32>,
    ) -> Result<(ByteOrder, u32), ArrowError> {
        if depth > MAX_GEOMETRY_DEPTH {
            return Err(invalid_wkb("geometry nesting exceeds 64 levels"));
        }
        let order = match self.take(1)?[0] {
            0 => ByteOrder::Big,
            1 => ByteOrder::Little,
            _ => return Err(invalid_wkb("invalid byte-order marker")),
        };
        let kind = self.u32(order)?;
        if !(WKB_POINT..=WKB_GEOMETRYCOLLECTION).contains(&kind) {
            return Err(invalid_wkb("unsupported geometry type"));
        }
        if expected.is_some_and(|expected| expected != kind) {
            return Err(invalid_wkb("nested geometry type mismatch"));
        }
        Ok((order, kind))
    }

    fn coord(
        &mut self,
        order: ByteOrder,
        path: &str,
        index: Option<usize>,
    ) -> Result<Coord2D, ArrowError> {
        let x = self.f64(order)?;
        if !x.is_finite() {
            return Err(nonfinite_coord(path, index, "x"));
        }
        let y = self.f64(order)?;
        if !y.is_finite() {
            return Err(nonfinite_coord(path, index, "y"));
        }
        Ok([x, y])
    }

    fn coords(&mut self, order: ByteOrder, path: &str) -> Result<Vec<Coord2D>, ArrowError> {
        let count = self.count(order, 16)?;
        let mut coords = Vec::new();
        for index in 0..count {
            coords.push(self.coord(order, path, Some(index))?);
        }
        Ok(coords)
    }

    fn rings(&mut self, order: ByteOrder, path: &str) -> Result<Vec<Vec<Coord2D>>, ArrowError> {
        let count = self.count(order, 4)?;
        let mut rings = Vec::new();
        for index in 0..count {
            rings.push(self.coords(order, &format!("{path}[{index}]"))?);
        }
        Ok(rings)
    }
}

fn invalid_wkb(reason: &str) -> ArrowError {
    ArrowError::ParseError(format!("invalid WKB: {reason}"))
}

fn nonfinite_coord(path: &str, index: Option<usize>, axis: &str) -> ArrowError {
    let location = match index {
        Some(index) => format!("{path}[{index}].{axis}"),
        None => format!("{path}.{axis}"),
    };
    invalid_wkb(&format!("non-finite coordinate at {location}"))
}

fn geometry_name(kind: u32) -> &'static str {
    match kind {
        WKB_POINT => "Point",
        WKB_LINESTRING => "LineString",
        WKB_POLYGON => "Polygon",
        WKB_MULTIPOINT => "MultiPoint",
        WKB_MULTILINESTRING => "MultiLineString",
        WKB_MULTIPOLYGON => "MultiPolygon",
        WKB_GEOMETRYCOLLECTION => "GeometryCollection",
        _ => unreachable!("header validates geometry types"),
    }
}

fn read_geometry(
    reader: &mut WkbReader<'_>,
    srid: i32,
    depth: usize,
    expected: Option<u32>,
    allow_point: bool,
    parent_path: &str,
) -> Result<CypherValue, ArrowError> {
    let (order, kind) = reader.header(depth, expected)?;
    let path = if parent_path.is_empty() {
        geometry_name(kind).to_owned()
    } else {
        format!("{parent_path}.{}", geometry_name(kind))
    };
    let geometry = match kind {
        WKB_POINT => {
            let [x, y] = reader.coord(order, &path, None)?;
            if !allow_point {
                // GeometryCollection has no Point child variant, but still validate its WKB.
                return Err(invalid_wkb(&format!(
                    "{path}: Point in GeometryCollection has no GeometryValue representation"
                )));
            }
            return Ok(CypherValue::Point(PointValue {
                srid,
                x,
                y,
                z: None,
            }));
        }
        WKB_LINESTRING => GeometryValue::LineString {
            coordinates: reader.coords(order, &path)?,
            srid,
        },
        WKB_POLYGON => GeometryValue::Polygon {
            rings: reader.rings(order, &path)?,
            srid,
        },
        WKB_MULTIPOINT => {
            let count = reader.count(order, 21)?;
            let mut points = Vec::new();
            for index in 0..count {
                let (nested_order, _) = reader.header(depth + 1, Some(WKB_POINT))?;
                points.push(reader.coord(nested_order, &path, Some(index))?);
            }
            GeometryValue::MultiPoint { points, srid }
        }
        WKB_MULTILINESTRING => {
            let count = reader.count(order, 9)?;
            let mut lines = Vec::new();
            for index in 0..count {
                let (nested_order, _) = reader.header(depth + 1, Some(WKB_LINESTRING))?;
                lines.push(reader.coords(nested_order, &format!("{path}[{index}]"))?);
            }
            GeometryValue::MultiLineString { lines, srid }
        }
        WKB_MULTIPOLYGON => {
            let count = reader.count(order, 9)?;
            let mut polygons = Vec::new();
            for index in 0..count {
                let (nested_order, _) = reader.header(depth + 1, Some(WKB_POLYGON))?;
                polygons.push(reader.rings(nested_order, &format!("{path}[{index}]"))?);
            }
            GeometryValue::MultiPolygon { polygons, srid }
        }
        WKB_GEOMETRYCOLLECTION => {
            let count = reader.count(order, 9)?;
            let mut geometries = Vec::new();
            for index in 0..count {
                match read_geometry(
                    reader,
                    srid,
                    depth + 1,
                    None,
                    false,
                    &format!("{path}[{index}]"),
                )? {
                    CypherValue::Geometry(geometry) => geometries.push(geometry),
                    _ => return Err(invalid_wkb("invalid GeometryCollection child")),
                }
            }
            GeometryValue::GeometryCollection { geometries, srid }
        }
        _ => return Err(invalid_wkb("unsupported geometry type")),
    };
    Ok(CypherValue::Geometry(geometry))
}

/// Decode a complete 2D WKB geometry with the given column SRID.
/// Rejects malformed headers, counts, nesting beyond 64 levels, and trailing bytes.
///
/// ```
/// use relate_arrow::geo_encoding::{wkb_array_to_geometries, wkb_to_cypher_value};
/// use relate_model::CypherValue;
/// use arrow_array::{BinaryArray, Int32Array};
///
/// let mut point = vec![0, 0, 0, 0, 1]; // big-endian Point header
/// point.extend_from_slice(&1.5_f64.to_be_bytes());
/// point.extend_from_slice(&(-2.25_f64).to_be_bytes());
/// assert!(matches!(wkb_to_cypher_value(&point, 4326)?,
///     CypherValue::Point(p) if p.srid == 4326 && p.x == 1.5 && p.y == -2.25));
/// let cells = BinaryArray::from(vec![Some(point.as_slice()), None]);
/// assert!(matches!(wkb_array_to_geometries(&cells, 4326)?[1], CypherValue::Null));
/// assert!(wkb_to_cypher_value(&point[..5], 4326).is_err());
/// assert!(wkb_array_to_geometries(&Int32Array::from(vec![1]), 4326).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn wkb_to_cypher_value(data: &[u8], srid: i32) -> Result<CypherValue, ArrowError> {
    let mut reader = WkbReader { data, offset: 0 };
    let value = read_geometry(&mut reader, srid, 1, None, true, "")?;
    if reader.offset != data.len() {
        return Err(invalid_wkb("trailing bytes"));
    }
    Ok(value)
}

/// Encode a slice of CypherValue geometries to an Arrow BinaryArray of WKB.
pub fn geometries_to_wkb_array(values: &[CypherValue]) -> ArrayRef {
    let wkb_values: Vec<Option<Vec<u8>>> = values.iter().map(cypher_value_to_wkb).collect();
    let refs: Vec<Option<&[u8]>> = wkb_values.iter().map(|v| v.as_deref()).collect();
    Arc::new(BinaryArray::from(refs))
}

/// Decode an Arrow BinaryArray of WKB back to CypherValues.
/// Null cells remain Null; malformed non-null cells and other array types error.
pub fn wkb_array_to_geometries(
    array: &dyn Array,
    srid: i32,
) -> Result<Vec<CypherValue>, ArrowError> {
    let binary = array
        .as_any()
        .downcast_ref::<BinaryArray>()
        .ok_or_else(|| ArrowError::CastError("expected BinaryArray for WKB".into()))?;
    (0..binary.len())
        .map(|i| {
            if binary.is_null(i) {
                Ok(CypherValue::Null)
            } else {
                wkb_to_cypher_value(binary.value(i), srid)
                    .map_err(|err| ArrowError::ParseError(format!("WKB row {i}: {err}")))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Int32Array;

    fn header(order: u8, kind: u32) -> Vec<u8> {
        let mut bytes = vec![order];
        bytes.extend_from_slice(&if order == 0 {
            kind.to_be_bytes()
        } else {
            kind.to_le_bytes()
        });
        bytes
    }

    fn append_count(bytes: &mut Vec<u8>, order: u8, count: u32) {
        bytes.extend_from_slice(&if order == 0 {
            count.to_be_bytes()
        } else {
            count.to_le_bytes()
        });
    }

    fn append_coord(bytes: &mut Vec<u8>, order: u8, x: f64, y: f64) {
        for value in [x, y] {
            bytes.extend_from_slice(&if order == 0 {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
    }

    fn external_point(order: u8, x: f64, y: f64) -> Vec<u8> {
        let mut bytes = header(order, WKB_POINT);
        append_coord(&mut bytes, order, x, y);
        bytes
    }

    fn external_line(order: u8, x: f64, y: f64) -> Vec<u8> {
        let mut bytes = header(order, WKB_LINESTRING);
        append_count(&mut bytes, order, 1);
        append_coord(&mut bytes, order, x, y);
        bytes
    }

    fn external_polygon(order: u8, x: f64, y: f64) -> Vec<u8> {
        let mut bytes = header(order, WKB_POLYGON);
        append_count(&mut bytes, order, 1);
        append_count(&mut bytes, order, 1);
        append_coord(&mut bytes, order, x, y);
        bytes
    }

    fn external_container(order: u8, kind: u32, children: &[&[u8]]) -> Vec<u8> {
        let mut bytes = header(order, kind);
        append_count(&mut bytes, order, children.len() as u32);
        for child in children {
            bytes.extend_from_slice(child);
        }
        bytes
    }

    fn assert_nonfinite_wkb_error(bytes: &[u8], coordinate_path: &str) {
        let err = wkb_to_cypher_value(bytes, 4326).unwrap_err();
        assert!(matches!(err, ArrowError::ParseError(_)), "{err}");
        assert!(err.to_string().contains(coordinate_path), "{err}");

        let finite = external_point(1, 1.5, -2.25);
        let cells = BinaryArray::from(vec![None, Some(finite.as_slice()), Some(bytes)]);
        let err = wkb_array_to_geometries(&cells, 4326).unwrap_err();
        assert!(matches!(err, ArrowError::ParseError(_)), "{err}");
        assert!(err.to_string().contains("WKB row 2"), "{err}");
        assert!(err.to_string().contains(coordinate_path), "{err}");
    }

    #[test]
    fn external_nonfinite_points_and_empty_sentinel_error() {
        for order in [0, 1] {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_nonfinite_wkb_error(&external_point(order, value, 2.0), "Point.x");
                assert_nonfinite_wkb_error(&external_point(order, 1.0, value), "Point.y");
            }
            // Standard WKB Point EMPTY uses two NaN coordinates; Cypher has no EMPTY Point.
            assert_nonfinite_wkb_error(&external_point(order, f64::NAN, f64::NAN), "Point.x");
        }
    }

    #[test]
    fn external_nonfinite_lines_and_polygons_error() {
        for order in [0, 1] {
            assert_nonfinite_wkb_error(&external_line(order, f64::NAN, 2.0), "LineString[0].x");
            assert_nonfinite_wkb_error(
                &external_line(order, 1.0, f64::INFINITY),
                "LineString[0].y",
            );
            assert_nonfinite_wkb_error(
                &external_polygon(order, f64::NEG_INFINITY, 2.0),
                "Polygon[0][0].x",
            );
            assert_nonfinite_wkb_error(&external_polygon(order, 1.0, f64::NAN), "Polygon[0][0].y");
        }
    }

    #[test]
    fn external_nonfinite_multi_members_error() {
        for order in [0, 1] {
            let nested_order = 1 - order;
            let point = external_point(nested_order, f64::INFINITY, 2.0);
            assert_nonfinite_wkb_error(
                &external_container(order, WKB_MULTIPOINT, &[&point]),
                "MultiPoint[0].x",
            );

            let line = external_line(nested_order, 1.0, f64::NEG_INFINITY);
            assert_nonfinite_wkb_error(
                &external_container(order, WKB_MULTILINESTRING, &[&line]),
                "MultiLineString[0][0].y",
            );

            let polygon = external_polygon(nested_order, f64::NAN, 2.0);
            assert_nonfinite_wkb_error(
                &external_container(order, WKB_MULTIPOLYGON, &[&polygon]),
                "MultiPolygon[0][0][0].x",
            );
        }
    }

    #[test]
    fn external_nonfinite_nested_collections_error() {
        for order in [0, 1] {
            let finite = external_line(order, 1.5, -2.25);
            let polygon = external_polygon(1 - order, 1.0, f64::INFINITY);
            let multi = external_container(order, WKB_MULTIPOLYGON, &[&polygon]);
            let inner = external_container(1 - order, WKB_GEOMETRYCOLLECTION, &[&multi]);
            let outer = external_container(order, WKB_GEOMETRYCOLLECTION, &[&finite, &inner]);
            assert_nonfinite_wkb_error(
                &outer,
                "GeometryCollection[1].GeometryCollection[0].MultiPolygon[0][0][0].y",
            );
        }
    }

    #[test]
    fn external_nonfinite_direct_collection_points_report_coordinate_path() {
        for collection_order in [0, 1] {
            for point_order in [0, 1] {
                let point = external_point(point_order, f64::NAN, 2.0);
                let collection =
                    external_container(collection_order, WKB_GEOMETRYCOLLECTION, &[&point]);
                assert_nonfinite_wkb_error(&collection, "GeometryCollection[0].Point.x");

                let point = external_point(point_order, 1.0, f64::INFINITY);
                let collection =
                    external_container(collection_order, WKB_GEOMETRYCOLLECTION, &[&point]);
                assert_nonfinite_wkb_error(&collection, "GeometryCollection[0].Point.y");
            }
        }
    }

    #[test]
    fn finite_point_collection_child_still_errors_with_location() {
        for collection_order in [0, 1] {
            for point_order in [0, 1] {
                let point = external_point(point_order, 1.5, -2.25);
                let collection =
                    external_container(collection_order, WKB_GEOMETRYCOLLECTION, &[&point]);
                let err = wkb_to_cypher_value(&collection, 4326).unwrap_err();
                assert!(matches!(err, ArrowError::ParseError(_)), "{err}");
                assert!(
                    err.to_string().contains("GeometryCollection[0].Point"),
                    "{err}"
                );
                assert!(
                    err.to_string().contains(
                        "Point in GeometryCollection has no GeometryValue representation"
                    ),
                    "{err}"
                );

                let array = BinaryArray::from(vec![None, Some(collection.as_slice())]);
                let err = wkb_array_to_geometries(&array, 4326).unwrap_err();
                assert!(err.to_string().contains("WKB row 1"), "{err}");
                assert!(
                    err.to_string().contains("GeometryCollection[0].Point"),
                    "{err}"
                );

                let truncated = &collection[..collection.len() - 1];
                let err = wkb_to_cypher_value(truncated, 4326).unwrap_err();
                assert!(err.to_string().contains("truncated geometry"), "{err}");
            }
        }
    }

    #[test]
    fn big_endian_point_and_nested_multipoint() {
        let mut point = header(0, WKB_POINT);
        point.extend_from_slice(&1.5f64.to_be_bytes());
        point.extend_from_slice(&(-2.25f64).to_be_bytes());
        assert_eq!(
            wkb_to_cypher_value(&point, 4326).unwrap(),
            CypherValue::Point(PointValue {
                srid: 4326,
                x: 1.5,
                y: -2.25,
                z: None,
            })
        );

        let mut multi = header(1, WKB_MULTIPOINT);
        multi.extend_from_slice(&1u32.to_le_bytes());
        multi.extend_from_slice(&point);
        assert_eq!(
            wkb_to_cypher_value(&multi, 4326).unwrap(),
            CypherValue::Geometry(GeometryValue::MultiPoint {
                points: vec![[1.5, -2.25]],
                srid: 4326,
            })
        );
    }

    #[test]
    fn nested_type_mismatch_is_rejected() {
        let mut multi = header(1, WKB_MULTIPOINT);
        multi.extend_from_slice(&1u32.to_le_bytes());
        multi.extend_from_slice(&header(1, WKB_LINESTRING));
        multi.extend_from_slice(&1.5f64.to_le_bytes());
        multi.extend_from_slice(&(-2.25f64).to_le_bytes());
        assert!(wkb_to_cypher_value(&multi, 4326).is_err());
    }

    #[test]
    fn collection_emitted_by_encoder_roundtrips() {
        let geom = GeometryValue::GeometryCollection {
            geometries: vec![
                GeometryValue::LineString {
                    coordinates: vec![[1.0, 2.0]],
                    srid: 4326,
                },
                GeometryValue::GeometryCollection {
                    geometries: vec![GeometryValue::Polygon {
                        rings: vec![vec![[3.0, 4.0]]],
                        srid: 4326,
                    }],
                    srid: 4326,
                },
            ],
            srid: 4326,
        };
        assert_eq!(
            wkb_to_cypher_value(&geometry_to_wkb(&geom), 4326).unwrap(),
            CypherValue::Geometry(geom)
        );
    }

    #[test]
    fn count_bombs_fail_before_reserving() {
        for kind in [
            WKB_LINESTRING,
            WKB_POLYGON,
            WKB_MULTIPOINT,
            WKB_MULTILINESTRING,
            WKB_MULTIPOLYGON,
            WKB_GEOMETRYCOLLECTION,
        ] {
            let mut bytes = header(1, kind);
            bytes.extend_from_slice(&u32::MAX.to_le_bytes());
            assert!(wkb_to_cypher_value(&bytes, 0).is_err(), "type {kind}");
        }
        let mut polygon = header(1, WKB_POLYGON);
        polygon.extend_from_slice(&1u32.to_le_bytes());
        polygon.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(wkb_to_cypher_value(&polygon, 0).is_err());
    }

    #[test]
    fn malformed_headers_truncation_and_trailing_bytes_error() {
        let mut unsupported = header(1, 99);
        assert!(wkb_to_cypher_value(&unsupported, 0).is_err());
        unsupported[0] = 0;
        assert!(wkb_to_cypher_value(&unsupported, 0).is_err());
        let mut point = point_to_wkb(&PointValue::cartesian_2d(1.0, 2.0));
        point[0] = 2;
        assert!(wkb_to_cypher_value(&point, 0).is_err());
        point[0] = 1;
        assert!(wkb_to_cypher_value(&point[..20], 0).is_err());
        point.push(0);
        assert!(wkb_to_cypher_value(&point, 0).is_err());

        for parent in [WKB_MULTIPOINT, WKB_MULTILINESTRING, WKB_MULTIPOLYGON] {
            let mut bytes = header(1, parent);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&header(2, WKB_POINT));
            bytes.extend_from_slice(&[0; 16]);
            assert!(wkb_to_cypher_value(&bytes, 0).is_err(), "type {parent}");
            bytes[10] = 1;
            assert!(wkb_to_cypher_value(&bytes, 0).is_err(), "type {parent}");
        }
        let mut collection = header(1, WKB_GEOMETRYCOLLECTION);
        collection.extend_from_slice(&1u32.to_le_bytes());
        collection.extend_from_slice(&point_to_wkb(&PointValue::cartesian_2d(1.0, 2.0)));
        assert!(wkb_to_cypher_value(&collection, 0).is_err());
        let mut collection = header(0, WKB_GEOMETRYCOLLECTION);
        collection.extend_from_slice(&1u32.to_be_bytes());
        collection.extend_from_slice(&header(1, WKB_LINESTRING));
        collection.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            wkb_to_cypher_value(&collection, 0).unwrap(),
            CypherValue::Geometry(GeometryValue::GeometryCollection {
                geometries: vec![GeometryValue::LineString {
                    coordinates: vec![],
                    srid: 0,
                }],
                srid: 0,
            })
        );
    }

    #[test]
    fn nested_byte_order_applies_to_counts_and_coordinates() {
        let mut multi = header(1, WKB_MULTILINESTRING);
        multi.extend_from_slice(&1u32.to_le_bytes());
        multi.extend_from_slice(&header(0, WKB_LINESTRING));
        multi.extend_from_slice(&1u32.to_be_bytes());
        multi.extend_from_slice(&1.5f64.to_be_bytes());
        multi.extend_from_slice(&(-2.25f64).to_be_bytes());
        assert_eq!(
            wkb_to_cypher_value(&multi, 4326).unwrap(),
            CypherValue::Geometry(GeometryValue::MultiLineString {
                lines: vec![vec![[1.5, -2.25]]],
                srid: 4326,
            })
        );
        let mut multi = header(0, WKB_MULTIPOLYGON);
        multi.extend_from_slice(&1u32.to_be_bytes());
        multi.extend_from_slice(&header(1, WKB_POLYGON));
        multi.extend_from_slice(&1u32.to_le_bytes());
        multi.extend_from_slice(&1u32.to_le_bytes());
        multi.extend_from_slice(&1.5f64.to_le_bytes());
        multi.extend_from_slice(&(-2.25f64).to_le_bytes());
        assert_eq!(
            wkb_to_cypher_value(&multi, 4326).unwrap(),
            CypherValue::Geometry(GeometryValue::MultiPolygon {
                polygons: vec![vec![vec![[1.5, -2.25]]]],
                srid: 4326,
            })
        );
    }

    #[test]
    fn collection_depth_is_bounded() {
        fn nested_collections(depth: usize) -> Vec<u8> {
            let mut bytes = Vec::new();
            for _ in 1..depth {
                bytes.extend_from_slice(&header(1, WKB_GEOMETRYCOLLECTION));
                bytes.extend_from_slice(&1u32.to_le_bytes());
            }
            bytes.extend_from_slice(&header(1, WKB_LINESTRING));
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes
        }
        assert!(wkb_to_cypher_value(&nested_collections(64), 0).is_ok());
        assert!(wkb_to_cypher_value(&nested_collections(65), 0).is_err());
    }

    #[test]
    fn array_rejects_wrong_type_and_invalid_non_null_cells() {
        assert!(wkb_array_to_geometries(&Int32Array::from(vec![1]), 4326).is_err());
        let valid = point_to_wkb(&PointValue::cartesian_2d(1.0, 2.0));
        let array = BinaryArray::from(vec![None, Some(valid.as_slice())]);
        let decoded = wkb_array_to_geometries(&array, 4326).unwrap();
        assert_eq!(decoded[0], CypherValue::Null);
        assert!(matches!(decoded[1], CypherValue::Point(_)));
        let array = BinaryArray::from(vec![None, Some(&b"invalid"[..])]);
        assert!(wkb_array_to_geometries(&array, 4326).is_err());
    }

    #[test]
    fn test_point_wkb_roundtrip() {
        let pt = PointValue::cartesian_2d(1.5, 2.5);
        let wkb = point_to_wkb(&pt);
        let decoded = wkb_to_cypher_value(&wkb, 7203).unwrap();
        match decoded {
            CypherValue::Point(p) => {
                assert!((p.x - 1.5).abs() < 1e-10);
                assert!((p.y - 2.5).abs() < 1e-10);
                assert_eq!(p.srid, 7203);
            }
            _ => panic!("expected Point"),
        }
    }

    #[test]
    fn test_linestring_wkb_roundtrip() {
        let geom = GeometryValue::LineString {
            coordinates: vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
            srid: 4326,
        };
        let wkb = geometry_to_wkb(&geom);
        let decoded = wkb_to_cypher_value(&wkb, 4326).unwrap();
        match decoded {
            CypherValue::Geometry(GeometryValue::LineString { coordinates, srid }) => {
                assert_eq!(coordinates.len(), 3);
                assert!((coordinates[1][0] - 1.0).abs() < 1e-10);
                assert_eq!(srid, 4326);
            }
            _ => panic!("expected LineString"),
        }
    }

    #[test]
    fn test_polygon_wkb_roundtrip() {
        let geom = GeometryValue::Polygon {
            rings: vec![vec![
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 4.0],
                [0.0, 4.0],
                [0.0, 0.0],
            ]],
            srid: 4326,
        };
        let wkb = geometry_to_wkb(&geom);
        let decoded = wkb_to_cypher_value(&wkb, 4326).unwrap();
        match decoded {
            CypherValue::Geometry(GeometryValue::Polygon { rings, srid }) => {
                assert_eq!(rings.len(), 1);
                assert_eq!(rings[0].len(), 5);
                assert_eq!(srid, 4326);
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn test_multipoint_wkb_roundtrip() {
        let geom = GeometryValue::MultiPoint {
            points: vec![[1.0, 2.0], [3.0, 4.0]],
            srid: 4326,
        };
        let wkb = geometry_to_wkb(&geom);
        let decoded = wkb_to_cypher_value(&wkb, 4326).unwrap();
        match decoded {
            CypherValue::Geometry(GeometryValue::MultiPoint { points, srid }) => {
                assert_eq!(points.len(), 2);
                assert!((points[0][0] - 1.0).abs() < 1e-10);
                assert_eq!(srid, 4326);
            }
            _ => panic!("expected MultiPoint"),
        }
    }

    #[test]
    fn test_geometries_to_wkb_array_roundtrip() {
        let values = vec![
            CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0)),
            CypherValue::Geometry(GeometryValue::LineString {
                coordinates: vec![[0.0, 0.0], [1.0, 1.0]],
                srid: 4326,
            }),
        ];
        let array = geometries_to_wkb_array(&values);
        let decoded = wkb_array_to_geometries(array.as_ref(), 4326).unwrap();
        assert_eq!(decoded.len(), 2);
        match &decoded[0] {
            CypherValue::Point(p) => assert!((p.x - 1.0).abs() < 1e-10),
            _ => panic!("expected Point"),
        }
        match &decoded[1] {
            CypherValue::Geometry(GeometryValue::LineString { coordinates, .. }) => {
                assert_eq!(coordinates.len(), 2);
            }
            _ => panic!("expected LineString"),
        }
    }

    #[test]
    fn test_non_spatial_returns_none() {
        assert!(cypher_value_to_wkb(&CypherValue::Int(42)).is_none());
        assert!(cypher_value_to_wkb(&CypherValue::Str("hello".into())).is_none());
    }
}
