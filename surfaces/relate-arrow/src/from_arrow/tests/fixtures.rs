use super::super::*;
use crate::record_batch::result_to_record_batch;
use arrow_array::ArrayRef;
use arrow_schema::Schema;
use std::{collections::HashMap, sync::Arc};

pub(super) fn batch(field: Field, array: ArrayRef) -> RecordBatch {
    RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![array]).unwrap()
}

pub(super) fn with_metadata(field: &Field, key: &str, value: &str) -> Field {
    field
        .clone()
        .with_metadata(HashMap::from([(key.into(), value.into())]))
}

fn edit_struct_batch(
    source: &RecordBatch,
    edit: impl FnOnce(&mut Vec<Field>, &mut Vec<ArrayRef>),
) -> RecordBatch {
    let original = source
        .column(0)
        .as_any()
        .downcast_ref::<StructArray>()
        .unwrap();
    let mut fields = original
        .fields()
        .iter()
        .map(|f| f.as_ref().clone())
        .collect::<Vec<_>>();
    let mut children = original.columns().to_vec();
    edit(&mut fields, &mut children);
    let edited =
        Arc::new(StructArray::try_new(fields.into(), children, original.nulls().cloned()).unwrap());
    batch(
        source
            .schema()
            .field(0)
            .clone()
            .with_data_type(edited.data_type().clone()),
        edited,
    )
}

pub(super) fn bad_child(source: &RecordBatch, name: &str, replacement: ArrayRef) -> RecordBatch {
    edit_struct_batch(source, |fields, children| {
        let pos = fields.iter().position(|f| f.name() == name).unwrap();
        fields[pos] = Field::new(name, replacement.data_type().clone(), true);
        children[pos] = replacement;
    })
}

pub(super) fn appended_child(source: &RecordBatch, name: &str, extra: ArrayRef) -> RecordBatch {
    edit_struct_batch(source, |fields, children| {
        fields.push(Field::new(name, extra.data_type().clone(), true));
        children.push(extra);
    })
}

pub(super) fn missing_child(source: &RecordBatch, name: &str) -> RecordBatch {
    edit_struct_batch(source, |fields, children| {
        let index = fields
            .iter()
            .position(|field| field.name() == name)
            .unwrap();
        fields.remove(index);
        children.remove(index);
    })
}

pub(super) fn one_col(
    col: &str,
    vals: Vec<CypherValue>,
) -> (Vec<String>, Vec<IndexMap<String, CypherValue>>) {
    let rows = vals
        .into_iter()
        .map(|v| {
            let mut m = IndexMap::new();
            m.insert(col.to_string(), v);
            m
        })
        .collect();
    (vec![col.to_string()], rows)
}

pub(super) fn roundtrip(vals: Vec<CypherValue>) -> Vec<CypherValue> {
    let (cols, rows) = one_col("x", vals);
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    let (_c, out) = record_batch_to_rows(&batch).unwrap();
    out.into_iter()
        .map(|mut r| r.swap_remove("x").unwrap())
        .collect()
}

pub(super) fn node(id: i64, label: &str, name: &str) -> CypherValue {
    let mut props = IndexMap::new();
    props.insert("name".to_string(), CypherValue::Str(name.into()));
    CypherValue::Node {
        id,
        labels: vec![label.into()],
        properties: props,
    }
}

pub(super) fn rel(id: i64, start: i64, end: i64) -> CypherValue {
    let mut props = IndexMap::new();
    props.insert("since".to_string(), CypherValue::Int(2020));
    CypherValue::Relationship {
        id,
        start_id: start,
        end_id: end,
        rel_type: "KNOWS".into(),
        properties: props,
    }
}
