//! Load dataset graphs into a Relate connection via Cypher.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use relate::{Connection, Result, Temporal, Value};

use crate::fraud::FraudGraph;
use crate::identity::IdentityGraph;

const BATCH: usize = 200;

pub fn load_fraud(conn: &mut Connection, graph: &FraudGraph) -> Result<()> {
    let customers: Vec<Value> = graph
        .customers
        .iter()
        .map(|c| {
            map([
                ("id", Value::String(c.id.as_str().into())),
                ("name", Value::String(c.name.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &customers,
        "UNWIND $rows AS c CREATE (:Customer {id: c.id, name: c.name})",
    )?;

    let accounts: Vec<Value> = graph
        .accounts
        .iter()
        .map(|a| {
            let planted = a
                .planted
                .as_ref()
                .map(|s| Value::String(s.as_str().into()))
                .unwrap_or(Value::Null);
            map([
                ("id", Value::String(a.id.as_str().into())),
                ("planted", planted),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &accounts,
        "UNWIND $rows AS a CREATE (:Account {id: a.id, planted: a.planted})",
    )?;

    // Equality seeks need a RANGE index and CONTAINS needs TEXT. Created
    // after the nodes exist so the following MATCH loads and the workload
    // both read the built indexes.
    for cypher in [
        "CREATE RANGE INDEX FOR (a:Account) ON (a.id)",
        "CREATE RANGE INDEX FOR (c:Customer) ON (c.id)",
        "CREATE RANGE INDEX FOR (a:Account) ON (a.planted)",
        "CREATE TEXT INDEX FOR (c:Customer) ON (c.name)",
    ] {
        conn.query(cypher)?;
    }

    let owned: Vec<Value> = graph
        .ownership
        .iter()
        .map(|(a, c)| {
            map([
                ("a", Value::String(a.as_str().into())),
                ("c", Value::String(c.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &owned,
        "UNWIND $rows AS r MATCH (a:Account {id: r.a}) MATCH (c:Customer {id: r.c}) CREATE (a)-[:OWNED_BY]->(c)",
    )?;

    let txs: Vec<Value> = graph
        .transfers
        .iter()
        .map(|t| {
            map([
                ("id", Value::String(t.id.as_str().into())),
                ("from", Value::String(t.from.as_str().into())),
                ("to", Value::String(t.to.as_str().into())),
                ("amount", Value::Float(t.amount)),
                ("ts", Value::Int(t.ts)),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &txs,
        "UNWIND $rows AS t MATCH (a:Account {id: t.from}) MATCH (b:Account {id: t.to}) CREATE (a)-[:TRANSFER {id: t.id, amount: t.amount, ts: t.ts}]->(b)",
    )?;
    Ok(())
}

pub fn load_identity(conn: &mut Connection, graph: &IdentityGraph) -> Result<()> {
    let users: Vec<Value> = graph
        .users
        .iter()
        .map(|u| {
            let planted = u
                .planted
                .as_ref()
                .map(|s| Value::String(s.as_str().into()))
                .unwrap_or(Value::Null);
            let emails = Value::List(
                u.emails
                    .iter()
                    .map(|e| Value::String(e.as_str().into()))
                    .collect::<Vec<_>>()
                    .into(),
            );
            map([
                ("id", Value::String(u.id.as_str().into())),
                ("name", Value::String(u.name.as_str().into())),
                ("email", Value::String(u.email.as_str().into())),
                ("status", Value::String(u.status.as_str().into())),
                ("hired", date_value(&u.hired)),
                ("emails", emails),
                ("planted", planted),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &users,
        "UNWIND $rows AS u CREATE (:User {id: u.id, name: u.name, email: u.email, status: u.status, hired: u.hired, emails: u.emails, planted: u.planted})",
    )?;

    let groups: Vec<Value> = graph
        .groups
        .iter()
        .map(|g| {
            map([
                ("id", Value::String(g.id.as_str().into())),
                ("name", Value::String(g.name.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &groups,
        "UNWIND $rows AS g CREATE (:Group {id: g.id, name: g.name})",
    )?;

    let roles: Vec<Value> = graph
        .roles
        .iter()
        .map(|r| {
            map([
                ("id", Value::String(r.id.as_str().into())),
                ("name", Value::String(r.name.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &roles,
        "UNWIND $rows AS r CREATE (:Role {id: r.id, name: r.name})",
    )?;

    let perms: Vec<Value> = graph
        .permissions
        .iter()
        .map(|p| {
            map([
                ("id", Value::String(p.id.as_str().into())),
                ("name", Value::String(p.name.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &perms,
        "UNWIND $rows AS p CREATE (:Permission {id: p.id, name: p.name})",
    )?;

    let apps: Vec<Value> = graph
        .apps
        .iter()
        .map(|a| {
            map([
                ("id", Value::String(a.id.as_str().into())),
                ("name", Value::String(a.name.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &apps,
        "UNWIND $rows AS a CREATE (:App {id: a.id, name: a.name})",
    )?;

    let devices: Vec<Value> = graph
        .devices
        .iter()
        .map(|d| map([("id", Value::String(d.id.as_str().into()))]))
        .collect();
    unwind_create(
        conn,
        &devices,
        "UNWIND $rows AS d CREATE (:Device {id: d.id})",
    )?;

    let um: Vec<Value> = graph
        .user_memberships
        .iter()
        .map(|(u, g)| {
            map([
                ("u", Value::String(u.as_str().into())),
                ("g", Value::String(g.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &um,
        "UNWIND $rows AS r MATCH (u:User {id: r.u}) MATCH (g:Group {id: r.g}) CREATE (u)-[:MEMBER_OF]->(g)",
    )?;

    let gm: Vec<Value> = graph
        .group_memberships
        .iter()
        .map(|(child, parent)| {
            map([
                ("c", Value::String(child.as_str().into())),
                ("p", Value::String(parent.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &gm,
        "UNWIND $rows AS r MATCH (c:Group {id: r.c}) MATCH (p:Group {id: r.p}) CREATE (c)-[:MEMBER_OF]->(p)",
    )?;

    let ur: Vec<Value> = graph
        .user_roles
        .iter()
        .map(|g| {
            let until = g
                .until
                .as_ref()
                .map(|s| date_value(s))
                .unwrap_or(Value::Null);
            map([
                ("u", Value::String(g.user.as_str().into())),
                ("r", Value::String(g.role.as_str().into())),
                ("until", until),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &ur,
        "UNWIND $rows AS r MATCH (u:User {id: r.u}) MATCH (role:Role {id: r.r}) CREATE (u)-[:HAS_ROLE {until: r.until}]->(role)",
    )?;

    let gr: Vec<Value> = graph
        .group_roles
        .iter()
        .map(|(g, r)| {
            map([
                ("g", Value::String(g.as_str().into())),
                ("r", Value::String(r.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &gr,
        "UNWIND $rows AS r MATCH (g:Group {id: r.g}) MATCH (role:Role {id: r.r}) CREATE (g)-[:HAS_ROLE]->(role)",
    )?;

    let grants: Vec<Value> = graph
        .grants
        .iter()
        .map(|(role, perm)| {
            map([
                ("r", Value::String(role.as_str().into())),
                ("p", Value::String(perm.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &grants,
        "UNWIND $rows AS r MATCH (role:Role {id: r.r}) MATCH (p:Permission {id: r.p}) CREATE (role)-[:GRANTS]->(p)",
    )?;

    let ons: Vec<Value> = graph
        .ons
        .iter()
        .map(|(perm, app)| {
            map([
                ("p", Value::String(perm.as_str().into())),
                ("a", Value::String(app.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &ons,
        "UNWIND $rows AS r MATCH (p:Permission {id: r.p}) MATCH (a:App {id: r.a}) CREATE (p)-[:ON]->(a)",
    )?;

    let reports: Vec<Value> = graph
        .reports_to
        .iter()
        .map(|(u, m)| {
            map([
                ("u", Value::String(u.as_str().into())),
                ("m", Value::String(m.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &reports,
        "UNWIND $rows AS r MATCH (u:User {id: r.u}) MATCH (m:User {id: r.m}) CREATE (u)-[:REPORTS_TO]->(m)",
    )?;

    let owns: Vec<Value> = graph
        .owns
        .iter()
        .map(|(u, d)| {
            map([
                ("u", Value::String(u.as_str().into())),
                ("d", Value::String(d.as_str().into())),
            ])
        })
        .collect();
    unwind_create(
        conn,
        &owns,
        "UNWIND $rows AS r MATCH (u:User {id: r.u}) MATCH (d:Device {id: r.d}) CREATE (u)-[:OWNS]->(d)",
    )?;
    Ok(())
}

fn date_value(iso: &str) -> Value {
    let d = NaiveDate::parse_from_str(iso, "%Y-%m-%d")
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2020, 1, 1).expect("fallback date"));
    Value::Temporal(Temporal::Date(d))
}

fn unwind_create(conn: &mut Connection, rows: &[Value], cypher: &str) -> Result<()> {
    for chunk in rows.chunks(BATCH) {
        let mut params = BTreeMap::new();
        params.insert("rows".into(), Value::List(chunk.to_vec().into()));
        conn.query_with_params(cypher, params)?;
    }
    Ok(())
}

fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Map(m.into())
}
