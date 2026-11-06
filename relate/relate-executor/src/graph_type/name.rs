//! Neo4j's server-generated graph-type constraint names.
//!
//! Neo4j derives a graph type's constraint names as
//! `constraint_<8 hex digits>` from the constraint descriptor
//! (`SchemaNameUtil.generateName`): an incremental XXH64 over the entity
//! type, the schema-pattern-matching feature, the entity/property token
//! *name* hashes, the constraint flag, and the kind-specific fields
//! (constraint type, backing index type, property-type set hash, endpoint
//! label, required label). The feature corpus asserts those names, so this
//! module reproduces the algorithm exactly; it is internal bookkeeping, not
//! a public naming contract.
//!
//! The property-type component hashes Neo4j's own `ConstrainableType`
//! serializations (`STRING`, `LIST_POINT`, `VECTOR[coordinate=…,
//! dimensions=…]`), summed over the set's members. A type outside that
//! vocabulary has no Neo4j spelling; [`derived_constraint_name`] falls back
//! to the descriptor's readable name there.

use relate_model::VectorCoordKind;
use relate_model::{ConstraintType, EntityKind};
use relate_parser::type_normalize::{CanonKind, CanonType};

/// The descriptor a name is generated for. Built through the per-kind
/// constructors below, so a descriptor whose payload does not match its
/// kind is unrepresentable.
pub(super) struct DerivedName<'a> {
    entity: EntityKind,
    label: &'a str,
    properties: &'a [String],
    kind: ConstraintType,
    payload: NamePayload<'a>,
}

/// The kind-specific fields the name hash consumes — the typed replacement
/// for three independent `Option`s whose `None` channel could not tell
/// "no such field for this kind" from "unsupported serialisation".
enum NamePayload<'a> {
    /// `unique`/`key`/`exists`: no kind-specific field.
    None,
    /// `propertyType`: the canonical type whose Neo4j serializations hash.
    PropertyType(&'a CanonType),
    /// `sourceLabel`/`targetLabel` (and their existence forms): the endpoint
    /// label. The side is the kind's, derived at the hash site.
    Endpoint { label: &'a str },
    /// `labelExistence`/`nodeLabelExistence`: the required label.
    RequiredLabel(&'a str),
}

impl<'a> DerivedName<'a> {
    /// The property-bearing kinds (`unique`/`key`/`exists`).
    pub(super) fn props(
        entity: EntityKind,
        label: &'a str,
        properties: &'a [String],
        kind: ConstraintType,
    ) -> Self {
        Self {
            entity,
            label,
            properties,
            kind,
            payload: NamePayload::None,
        }
    }

    /// A `propertyType` constraint, whose type joins the hash.
    pub(super) fn property_type(
        entity: EntityKind,
        label: &'a str,
        properties: &'a [String],
        property_type: &'a CanonType,
    ) -> Self {
        Self {
            entity,
            label,
            properties,
            kind: ConstraintType::PropertyType,
            payload: NamePayload::PropertyType(property_type),
        }
    }

    /// An edge endpoint-label constraint: the identifying relationship type,
    /// the typed kind, and the endpoint label. The endpoint side is derived
    /// from the kind, so a `TargetLabel` can never hash a source label.
    pub(super) fn endpoint(
        identifying_label: &'a str,
        kind: ConstraintType,
        endpoint_label: &'a str,
    ) -> Self {
        Self {
            entity: EntityKind::Relationship,
            label: identifying_label,
            properties: &[],
            kind,
            payload: NamePayload::Endpoint {
                label: endpoint_label,
            },
        }
    }

    /// A label-existence constraint: the identifying label and the label the
    /// type requires.
    pub(super) fn required_label(
        entity: EntityKind,
        label: &'a str,
        required_label: &'a str,
    ) -> Self {
        Self {
            entity,
            label,
            properties: &[],
            kind: ConstraintType::LabelExistence,
            payload: NamePayload::RequiredLabel(required_label),
        }
    }

    /// The readable fallback a generated name replaces — one spelling for
    /// every kind, so call sites pass typed inputs only.
    fn readable_fallback(&self) -> String {
        let suffix = match &self.payload {
            NamePayload::Endpoint { label } | NamePayload::RequiredLabel(label) => {
                (*label).to_string()
            }
            NamePayload::PropertyType(_) | NamePayload::None => self.properties.join("_"),
        };
        format!(
            "constraint_{}_{}_{}",
            self.kind.as_str(),
            self.label,
            suffix
        )
    }
}

/// The generated name, or the descriptor's readable fallback when the
/// property type has no Neo4j serialization.
pub(super) fn derived_constraint_name(spec: &DerivedName<'_>) -> String {
    generated_name(spec).unwrap_or_else(|| spec.readable_fallback())
}

fn generated_name(spec: &DerivedName<'_>) -> Option<String> {
    let entity = match spec.entity {
        EntityKind::Node => 0,
        EntityKind::Relationship => 1,
    };
    let shape = name_shape(spec.kind);
    // `SINGLE_ENTITY_TOKEN` (ordinal 3) for the label-only kinds,
    // `COMPLETE_ALL_TOKENS` (ordinal 0) for the property-bearing ones;
    // a single-property schema widens to the COMPLETE|PARTIAL feature mask.
    let spmt = if shape.label_only { 3 } else { 0 };
    let effective = if !shape.label_only && spec.properties.len() == 1 {
        3
    } else {
        1 << spmt
    };
    let mut state = Xxh64::initialise(SEED);
    state = update(state, entity);
    state = update(state, effective);
    state = update_array(state, &[java_string_hash(spec.label)]);
    let property_hashes = spec
        .properties
        .iter()
        .map(|property| java_string_hash(property))
        .collect::<Vec<_>>();
    state = update_array(state, &property_hashes);
    state = update(state, JAVA_TRUE_HASH);
    state = update(state, shape.ordinal);
    match (spec.kind, &spec.payload) {
        (ConstraintType::Unique | ConstraintType::Key, NamePayload::None) => {
            // The backing index is always a RANGE index (`IndexType.RANGE`
            // type number 4).
            state = update(state, 4);
        }
        (ConstraintType::PropertyType, NamePayload::PropertyType(property_type)) => {
            let members = serialize_members(property_type)?;
            let sum = members.iter().fold(0i64, |sum, member| {
                sum.wrapping_add(java_string_hash(member))
            });
            state = update(state, sum);
        }
        (
            ConstraintType::SourceLabel
            | ConstraintType::SourceLabelExistence
            | ConstraintType::TargetLabel
            | ConstraintType::TargetLabelExistence,
            NamePayload::Endpoint { label },
        ) => {
            // The endpoint side is the kind's: 1 for the target kinds, 0 for
            // the source kinds, so a `TargetLabel` can never hash a source
            // label.
            let target = matches!(
                spec.kind,
                ConstraintType::TargetLabel | ConstraintType::TargetLabelExistence
            );
            state = update(state, java_string_hash(label));
            state = update(state, if target { 1 } else { 0 });
        }
        (
            ConstraintType::LabelExistence | ConstraintType::NodeLabelExistence,
            NamePayload::RequiredLabel(required_label),
        ) => {
            state = update(state, java_string_hash(required_label));
        }
        (ConstraintType::Exists, NamePayload::None) => {}
        // A kind/payload mismatch is unrepresentable through the
        // constructors above; refuse to hash rather than fold two schemas
        // onto one name. A new kind fails `name_shape`'s exhaustive match
        // before it can reach this arm.
        _ => return None,
    }
    Some(format!("constraint_{:x}", to_int(finalise(state))))
}

/// The per-kind name-hash inputs that do not come from the payload: Neo4j's
/// `ConstraintType` ordinal, and whether the kind is label-only (its
/// schema-pattern-matching feature is `SINGLE_ENTITY_TOKEN`, ordinal 3,
/// rather than `COMPLETE_ALL_TOKENS`, 0). One exhaustive table: a new kind
/// fails the build here instead of inheriting a `false` default.
struct NameShape {
    ordinal: i64,
    label_only: bool,
}

fn name_shape(kind: ConstraintType) -> NameShape {
    match kind {
        ConstraintType::Unique => NameShape {
            ordinal: 0,
            label_only: false,
        },
        ConstraintType::Exists => NameShape {
            ordinal: 1,
            label_only: false,
        },
        ConstraintType::Key => NameShape {
            ordinal: 2,
            label_only: false,
        },
        ConstraintType::PropertyType => NameShape {
            ordinal: 3,
            label_only: false,
        },
        ConstraintType::SourceLabel
        | ConstraintType::SourceLabelExistence
        | ConstraintType::TargetLabel
        | ConstraintType::TargetLabelExistence => NameShape {
            ordinal: 4,
            label_only: true,
        },
        ConstraintType::LabelExistence | ConstraintType::NodeLabelExistence => NameShape {
            ordinal: 5,
            label_only: true,
        },
    }
}

/// The Neo4j `ConstrainableType` serializations of one canonical property
/// type: one entry per set member (a closed union contributes each member).
fn serialize_members(t: &CanonType) -> Option<Vec<String>> {
    match &t.kind {
        CanonKind::Predefined(kind) => Some(vec![predefined_serialization(*kind)?.to_string()]),
        CanonKind::List(element) => match &element.kind {
            CanonKind::Union(members) => members
                .iter()
                .map(list_serialization)
                .collect::<Option<Vec<_>>>(),
            _ => Some(vec![list_serialization(element)?]),
        },
        CanonKind::Vector { coord, dimension } => Some(vec![format!(
            "VECTOR[coordinate={}, dimensions={}]",
            vector_coord_name((*coord)?),
            (*dimension)?
        )]),
        CanonKind::Union(members) => members
            .iter()
            .map(serialize_members)
            .collect::<Option<Vec<Vec<_>>>>()
            .map(|groups| groups.into_iter().flatten().collect()),
    }
}

/// One list member's serialization: the scalar `LIST_*` spelling, or
/// Neo4j's `VectorArray` for a list of vectors.
fn list_serialization(t: &CanonType) -> Option<String> {
    match &t.kind {
        CanonKind::Predefined(kind) => Some(format!("LIST_{}", predefined_serialization(*kind)?)),
        CanonKind::Vector { .. } => Some("VectorArray".to_string()),
        _ => None,
    }
}

/// A predefined type's `SchemaValueType` name (spaces become underscores
/// for the temporal names).
fn predefined_serialization(kind: relate_parser::PredefinedKind) -> Option<&'static str> {
    use relate_parser::PredefinedKind as K;
    match kind {
        K::Boolean => Some("BOOLEAN"),
        K::String => Some("STRING"),
        K::Uuid => Some("UUID"),
        K::Integer => Some("INTEGER"),
        K::Float => Some("FLOAT"),
        K::Date => Some("DATE"),
        K::LocalTime => Some("LOCAL_TIME"),
        K::ZonedTime => Some("ZONED_TIME"),
        K::LocalDateTime => Some("LOCAL_DATETIME"),
        K::ZonedDateTime => Some("ZONED_DATETIME"),
        K::Duration => Some("DURATION"),
        K::Point => Some("POINT"),
        _ => None,
    }
}

/// A vector coordinate's Java enum name (`INTEGER64`, `FLOAT32`, …).
fn vector_coord_name(coord: VectorCoordKind) -> &'static str {
    match coord {
        VectorCoordKind::Int8 => "INTEGER8",
        VectorCoordKind::Int16 => "INTEGER16",
        VectorCoordKind::Int32 => "INTEGER32",
        VectorCoordKind::Int64 => "INTEGER64",
        VectorCoordKind::Float16 => "FLOAT16",
        VectorCoordKind::BFloat16 => "BFLOAT16",
        VectorCoordKind::Float32 => "FLOAT32",
        VectorCoordKind::Float64 => "FLOAT64",
    }
}

/// Java's `String.hashCode`, sign-extended to the `long` Neo4j's hash
/// update consumes.
fn java_string_hash(text: &str) -> i64 {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    i64::from(hash)
}

/// The seed Neo4j's `SchemaNameUtil.generateName` initialises with.
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// `Boolean.hashCode(true)`.
const JAVA_TRUE_HASH: i64 = 1231;
/// XXH64 constants: `PRIME64_5` (the initialise addend) and `PRIME64_1`/
/// `_2`/`_3`/`_4` (the update/finalise primes).
const PRIME64_5: u64 = 0x27D4_EB2F_1656_67C5;
const PRIME64_4: u64 = 0x85EB_CA77_C2B2_AE63;
const PRIME64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const PRIME64_1: u64 = 0x9E37_79B1_85EB_CA87;
const PRIME64_3: u64 = 0x1656_67B1_9E37_79F9;

/// The incremental XXH64 (`IncrementalXXH64`): `update` mixes one 8-byte
/// value into the running state, `finalise` runs the avalanche.
struct Xxh64;

impl Xxh64 {
    fn initialise(seed: u64) -> u64 {
        seed.wrapping_add(PRIME64_5)
    }
}

/// The `value as u64` cast is the sign-extension Java's `i2l` performs.
fn update(state: u64, value: i64) -> u64 {
    let mut state = state.wrapping_add(8);
    let mut value = (value as u64).wrapping_mul(PRIME64_2);
    value = value.rotate_left(31);
    value = value.wrapping_mul(PRIME64_1);
    state ^= value;
    state
        .rotate_left(27)
        .wrapping_mul(PRIME64_1)
        .wrapping_add(PRIME64_4)
}

fn update_array(state: u64, values: &[i64]) -> u64 {
    let mut state = update(state, values.len() as i64);
    for value in values {
        state = update(state, *value);
    }
    state
}

fn finalise(state: u64) -> u64 {
    let mut state = state;
    state ^= state >> 33;
    state = state.wrapping_mul(PRIME64_2);
    state ^= state >> 29;
    state = state.wrapping_mul(PRIME64_3);
    state ^= state >> 32;
    state
}

/// `HashFunction.toInt`: the low 32 bits of `state ^ (state >>> 32)`,
/// formatted unsigned (Java's `%x` on an `int`).
fn to_int(state: u64) -> u32 {
    ((state ^ (state >> 32)) & 0xFFFF_FFFF) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(text: &str) -> CanonType {
        let body = relate_parser::parse_graph_type_body(&format!("(n:L => {{p :: {text}}})"))
            .expect("graph type parses");
        relate_parser::type_expr_to_canon(&body.node_types[0].properties[0].value_type)
    }

    /// The names live Neo4j 2026.09 produced for a graph type containing
    /// `(:Label1 => :Label2 {prop :: STRING NOT NULL})`,
    /// `(:Label3)-[:REL_TYPE1 => {prop :: INT NOT NULL}]->(:Label4)` and the
    /// explicit `c1`-`c8` constraints' implied records. Each assertion builds
    /// the typed descriptor it means — the endpoint side comes from the kind,
    /// never from a test-only bool.
    #[test]
    fn names_match_live_neo4j() {
        assert_eq!(
            derived_constraint_name(&DerivedName::props(
                EntityKind::Node,
                "Label1",
                &["prop".to_string()],
                ConstraintType::Exists,
            )),
            "constraint_b75b74fc"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::property_type(
                EntityKind::Node,
                "Label1",
                &["prop".to_string()],
                &canon("STRING"),
            )),
            "constraint_f7912a5d"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::required_label(
                EntityKind::Node,
                "Label1",
                "Label2",
            )),
            "constraint_69e8c8ea"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::endpoint(
                "REL_TYPE1",
                ConstraintType::SourceLabel,
                "Label3",
            )),
            "constraint_219733e7"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::endpoint(
                "REL_TYPE1",
                ConstraintType::TargetLabel,
                "Label4",
            )),
            "constraint_f757795b"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::property_type(
                EntityKind::Relationship,
                "REL_TYPE1",
                &["prop".to_string()],
                &canon("INTEGER"),
            )),
            "constraint_7dec30b1"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::property_type(
                EntityKind::Node,
                "NA",
                &["p".to_string()],
                &canon("LIST<POINT NOT NULL>"),
            )),
            "constraint_9671d4a5"
        );
        assert_eq!(
            derived_constraint_name(&DerivedName::property_type(
                EntityKind::Node,
                "NA",
                &["p".to_string()],
                &canon("VECTOR<INTEGER NOT NULL>(3)"),
            )),
            "constraint_4382286b"
        );
    }

    /// A type outside the Neo4j vocabulary keeps the descriptor's readable
    /// fallback name rather than losing the constraint.
    #[test]
    fn unsupported_property_types_fall_back_to_the_readable_name() {
        let properties = ["p".to_string()];
        assert_eq!(
            derived_constraint_name(&DerivedName::property_type(
                EntityKind::Node,
                "NA",
                &properties,
                &canon("LIST<LIST<POINT NOT NULL> NOT NULL>"),
            )),
            "constraint_propertyType_NA_p"
        );
    }
}
