//! `Spatial` function rows: the `POINT` constructor, its distance and its
//! bounding-box test.

use super::{FunctionCategory, FunctionMeta};
use crate::catalog_meta::ProcArgMeta;

pub(super) const ENTRIES: &[FunctionMeta] = &[
    FunctionMeta::described(
        "point",
        FunctionCategory::Spatial,
        "Returns a 2D or 3D point object, given two or respectively three coordinate values in the Cartesian coordinate system or WGS 84 geographic coordinate system.",
        &[
            ProcArgMeta::described(
                "input",
                "Cartesian 2D: {x :: FLOAT, y :: FLOAT, crs = \"cartesian\" :: STRING, srid = 7203 :: INTEGER}\n\nCartesian 3D: {x :: FLOAT, y :: FLOAT, z :: FLOAT, crs = \"cartesian-3D\" :: STRING, srid = 9157 :: INTEGER} \n\nWGS 84 2D: {longitude | x :: FLOAT, latitude | y :: FLOAT, crs = \"WGS-84-2D\" :: STRING, srid = 4326 :: INTEGER}\n\nWGS 84 3D: {longitude | x :: FLOAT, latitude | y :: FLOAT, height | z :: FLOAT, crs = \"WGS-84-3D\" :: STRING, srid = 4979 :: INTEGER}",
                "MAP",
            ),
        ],
        "POINT",
    ),
    FunctionMeta::described(
        "point.distance",
        FunctionCategory::Spatial,
        "Returns a `FLOAT` representing the distance between any two points in the same CRS. If the points are in the WGS 84 CRS, the function returns the geodesic distance (i.e., the shortest path along the curved surface of the Earth). If the points are in a Cartesian CRS, the function returns the Euclidean distance (i.e., the shortest straight-line distance in a flat, planar space).",
        &[
            ProcArgMeta::described("from", "A start point.", "POINT"),
            ProcArgMeta::described(
                "to",
                "An end point in the same CRS as the start point.",
                "POINT",
            ),
        ],
        "FLOAT",
    ),
    FunctionMeta::described(
        "point.withinBBox",
        FunctionCategory::Spatial,
        "Returns true if the provided point is within the bounding box defined by the two provided points.",
        &[
            ProcArgMeta::described(
                "point",
                "A point to be confirmed in the bounding box.",
                "POINT",
            ),
            ProcArgMeta::described(
                "lowerLeft",
                "The lower left side point of the bounding box.",
                "POINT",
            ),
            ProcArgMeta::described(
                "upperRight",
                "The upper right side point of the bounding box.",
                "POINT",
            ),
        ],
        "BOOLEAN",
    ),
];
