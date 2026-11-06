mod common;

use relate::{Database, Value};
use relate_datasets::identity::{generate, IdentityConfig, ROOT_USER_ID};
use relate_datasets::load_identity;

use common::params;

#[test]
fn identity_ci_scale_detects_planted_patterns() {
    let cfg = IdentityConfig::ci();
    let graph = generate(&cfg);
    let ordinary = graph.users.iter().filter(|u| u.planted.is_none()).count();
    assert_eq!(ordinary, 500);
    assert!(graph.users.len() > ordinary);
    assert_eq!(graph.planted.nested_admin, "nested-admin");
    assert_eq!(graph.planted.nest_groups.len(), 4);
    // The nest-leaf fan joins the first nest group, `nest-0`.
    assert_eq!(graph.planted.fan_group, "nest-0");
    assert_eq!(graph.planted.leaf_users.len(), 12);
    assert_eq!(graph.planted.orphan_priv, "orphan-priv");
    assert_eq!(graph.planted.sod_user, "sod-user");
    assert_eq!(graph.planted.stale_grant, "stale-grant");
    let mut pairs = std::collections::BTreeSet::new();
    for (user, group) in &graph.user_memberships {
        assert!(
            pairs.insert((user.clone(), group.clone())),
            "duplicate membership {user} in {group}"
        );
    }
    assert!(
        graph
            .reports_to
            .iter()
            .all(|(user, manager)| user != manager),
        "a user must not report to themselves"
    );

    let db = Database::open(":memory:").unwrap();
    let mut c = db.connect().unwrap();
    load_identity(&mut c, &graph).expect("load identity");

    let nested = c
        .query_with_params(
            "MATCH (u:User {id: $id})-[:MEMBER_OF*1..8]->(g:Group)-[:HAS_ROLE]->(r:Role {name: 'admin'}) \
             RETURN DISTINCT u.id AS id",
            params([(
                "id",
                Value::String(graph.planted.nested_admin.as_str().into()),
            )]),
        )
        .expect("nested admin query");
    let ids: Vec<_> = nested
        .rows
        .iter()
        .filter_map(|r| r[0].as_string().map(|s| s.to_string()))
        .collect();
    assert!(
        ids.iter().any(|id| id == "nested-admin"),
        "planted nested-admin must inherit admin, got {ids:?}"
    );

    let orphan = c
        .query(
            "MATCH (u:User {id: 'orphan-priv'})-[:HAS_ROLE]->(:Role {name: 'admin'}) \
             OPTIONAL MATCH (u)-[:REPORTS_TO]->(m:User) \
             RETURN u.id AS id, m.id AS manager",
        )
        .expect("orphan query");
    assert_eq!(orphan.rows.len(), 1);
    assert_eq!(orphan.rows[0][0].as_string(), Some("orphan-priv"));
    assert!(
        matches!(orphan.rows[0][1], Value::Null),
        "orphan-priv must have no manager, got {:?}",
        orphan.rows[0][1]
    );

    let sod = c
        .query_with_params(
            "MATCH (u:User {id: $id}) \
             WHERE EXISTS { (u)-[:HAS_ROLE]->(:Role {name: 'requester'}) } \
               AND EXISTS { (u)-[:HAS_ROLE]->(:Role {name: 'approver'}) } \
             RETURN u.id AS id",
            params([("id", Value::String(graph.planted.sod_user.as_str().into()))]),
        )
        .expect("sod query");
    let sod_ids: Vec<_> = sod
        .rows
        .iter()
        .filter_map(|r| r[0].as_string().map(|s| s.to_string()))
        .collect();
    assert!(
        sod_ids.contains(&graph.planted.sod_user),
        "planted sod-user missing from {sod_ids:?}"
    );

    let stale = c
        .query(
            "MATCH (u:User)-[g:HAS_ROLE]->(:Role) WHERE g.until < date('2024-01-01') RETURN u.id AS id",
        )
        .expect("stale query");
    let stale_ids: Vec<_> = stale
        .rows
        .iter()
        .filter_map(|r| r[0].as_string().map(|s| s.to_string()))
        .collect();
    assert!(
        stale_ids.contains(&graph.planted.stale_grant),
        "planted stale-grant missing from {stale_ids:?}"
    );

    c.query_with_params(
        "MATCH (u:User) \
         WHERE u.email CONTAINS $q OR any(e IN u.emails WHERE e CONTAINS $q) \
         RETURN u.id AS id ORDER BY u.id LIMIT 5",
        params([("q", Value::String("example.com".into()))]),
    )
    .expect("search directory");
    c.query_with_params(
        "MATCH (g:Group {id: $gid})<-[:MEMBER_OF]-(u:User) \
         RETURN u.id AS id ORDER BY u.name SKIP $skip LIMIT 5",
        params([
            ("gid", Value::String("grp-0".into())),
            ("skip", Value::Int(0)),
        ]),
    )
    .expect("group roster");
    c.query_with_params(
        "MATCH (u:User {id: $id}) \
         RETURN CASE u.status WHEN 'active' THEN 'ok' ELSE 'blocked' END AS review",
        params([("id", Value::String(ROOT_USER_ID.into()))]),
    )
    .expect("access review");
    c.query_with_params(
        "MATCH (a:User {id: $from}), (b:User {id: $to}) \
         RETURN length(shortestPath((a)-[:REPORTS_TO*..8]-(b))) AS hops",
        params([
            ("from", Value::String("user-1".into())),
            ("to", Value::String(ROOT_USER_ID.into())),
        ]),
    )
    .expect("reporting path");
    c.query_with_params(
        "MATCH (u:User {id: $uid}) SET u.status = 'inactive'",
        params([("uid", Value::String("user-2".into()))]),
    )
    .expect("disable user");
    c.query_with_params(
        "MATCH (u:User {id: $uid}) SET u.emails = u.emails + [$alias]",
        params([
            ("uid", Value::String("user-3".into())),
            ("alias", Value::String("extra@example.com".into())),
        ]),
    )
    .expect("add alias");
    c.query_with_params(
        "MATCH (u:User {id: $uid})-[m:MEMBER_OF]->(g:Group {id: $gid}) DELETE m",
        params([
            ("uid", Value::String("sod-user".into())),
            ("gid", Value::String("grp-0".into())),
        ]),
    )
    .expect("revoke membership");
    c.query_with_params(
        "MATCH (u:User {id: $uid})-[g:HAS_ROLE]->() WHERE g.until IS NOT NULL REMOVE g.until",
        params([(
            "uid",
            Value::String(graph.planted.stale_grant.as_str().into()),
        )]),
    )
    .expect("clear expiry");
}

#[test]
#[ignore]
fn identity_full_scale_generates() {
    let g = generate(&IdentityConfig::default());
    assert_eq!(g.users.len(), 10_000 + 25 + 3);
    assert!(g.groups.len() > 100);
    assert!(!g.user_memberships.is_empty());
}
