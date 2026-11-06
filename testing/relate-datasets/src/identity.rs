//! Seeded synthetic enterprise-identity (IAM / directory) graph.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityGraph {
    pub seed: u64,
    pub scale: f64,
    pub users: Vec<IdentityUser>,
    pub groups: Vec<IdentityGroup>,
    pub roles: Vec<IdentityRole>,
    pub permissions: Vec<IdentityPermission>,
    pub apps: Vec<IdentityApp>,
    pub devices: Vec<IdentityDevice>,
    pub user_memberships: Vec<(String, String)>,
    pub group_memberships: Vec<(String, String)>,
    pub user_roles: Vec<UserRoleGrant>,
    pub group_roles: Vec<(String, String)>,
    pub grants: Vec<(String, String)>,
    pub ons: Vec<(String, String)>,
    pub reports_to: Vec<(String, String)>,
    pub owns: Vec<(String, String)>,
    pub planted: IdentityPlanted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityUser {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub hired: String,
    pub emails: Vec<String>,
    /// `nested-admin`, `orphan-priv`, `sod`, `stale`, `nest-leaf`, or omitted.
    pub planted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityGroup {
    pub id: String,
    pub name: String,
    /// `nest`, or omitted.
    pub planted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRole {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityPermission {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityApp {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDevice {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRoleGrant {
    pub user: String,
    pub role: String,
    /// ISO date `YYYY-MM-DD` when the grant expires; omitted if open-ended.
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityPlanted {
    pub nested_admin: String,
    pub nest_groups: Vec<String>,
    /// The group the nest-leaf fan joins.
    pub fan_group: String,
    pub nest_depth: i64,
    pub group_k: i64,
    pub leaf_users: Vec<String>,
    pub orphan_priv: String,
    pub sod_user: String,
    pub stale_grant: String,
    pub stale_until: String,
}

#[derive(Debug, Clone)]
pub struct IdentityConfig {
    pub seed: u64,
    pub scale: f64,
    pub nest_depth: usize,
    pub group_k: usize,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            scale: 1.0,
            nest_depth: 4,
            group_k: 25,
        }
    }
}

impl IdentityConfig {
    pub fn ci() -> Self {
        Self {
            scale: 0.05,
            nest_depth: 4,
            group_k: 12,
            ..Self::default()
        }
    }

    pub fn from_env() -> Self {
        if std::env::var("RELATE_FULL_DATA").ok().as_deref() == Some("1") {
            Self::default()
        } else {
            Self::ci()
        }
    }
}

/// Org root user. User 0 has no manager; planted users report to it.
pub const ROOT_USER_ID: &str = "user-0";

/// Role ids created by [`generate`]: the role catalog, the role→permission
/// grants, and every group and user role grant consult these consts instead
/// of re-spelling the ids.
pub(crate) const ROLE_VIEWER_ID: &str = "role-viewer";
pub(crate) const ROLE_EDITOR_ID: &str = "role-editor";
pub(crate) const ROLE_ADMIN_ID: &str = "role-admin";
pub(crate) const ROLE_REQUESTER_ID: &str = "role-requester";
pub(crate) const ROLE_APPROVER_ID: &str = "role-approver";

/// Role ids the identity workload's `grant_role` op samples, and the
/// root-group grants rotate over by parity. Viewer and editor only: they
/// are the roles granted at random, while admin, requester, and approver
/// mark the planted IAM patterns and stay out of random grants. The
/// generator's fixed ordinary-user grant spells [`ROLE_VIEWER_ID`]
/// directly.
pub(crate) const GRANTABLE_ROLE_IDS: &[&str] = &[ROLE_VIEWER_ID, ROLE_EDITOR_ID];

/// Permission ids created by [`generate`]: the permission catalog, the
/// role→permission grants, and the permission→app ons consult these consts
/// instead of re-spelling the ids.
pub(crate) const PERM_WIKI_READ_ID: &str = "perm-wiki-read";
pub(crate) const PERM_CRM_WRITE_ID: &str = "perm-crm-write";
pub(crate) const PERM_ADMIN_ALL_ID: &str = "perm-admin-all";
pub(crate) const PERM_PAYROLL_READ_ID: &str = "perm-payroll-read";

/// App ids created by [`generate`]: the app catalog and the permission→app
/// ons consult these consts instead of re-spelling the ids.
pub(crate) const APP_PAYROLL_ID: &str = "app-payroll";
pub(crate) const APP_CRM_ID: &str = "app-crm";
pub(crate) const APP_WIKI_ID: &str = "app-wiki";
pub(crate) const APP_ADMIN_ID: &str = "app-admin";

const STALE_UNTIL: &str = "2020-06-01";

pub fn generate(cfg: &IdentityConfig) -> IdentityGraph {
    let mut rng = StdRng::seed_from_u64(cfg.seed);
    let n_users = ((10_000.0 * cfg.scale).round() as usize).max(20);
    let n_groups = (n_users / 20).max(8);
    let nest_depth = cfg.nest_depth.clamp(2, 8);
    let group_k = cfg.group_k.max(3);

    let mut groups = Vec::new();
    for i in 0..n_groups {
        groups.push(IdentityGroup {
            id: format!("grp-{i}"),
            name: format!("Group {i}"),
            planted: None,
        });
    }

    let mut group_memberships = Vec::new();
    let n_roots = (n_groups / 5).max(2);
    for i in n_roots..n_groups {
        let parent = i % n_roots;
        group_memberships.push((format!("grp-{i}"), format!("grp-{parent}")));
    }

    let mut group_roles = Vec::new();
    for i in 0..n_roots {
        let role_id = GRANTABLE_ROLE_IDS[i % GRANTABLE_ROLE_IDS.len()];
        group_roles.push((format!("grp-{i}"), role_id.into()));
    }

    let mut nest_groups = Vec::new();
    for k in 0..nest_depth {
        let id = format!("nest-{k}");
        groups.push(IdentityGroup {
            id: id.clone(),
            name: format!("Nested {k}"),
            planted: Some("nest".into()),
        });
        nest_groups.push(id);
    }
    for k in 0..nest_depth.saturating_sub(1) {
        group_memberships.push((nest_groups[k].clone(), nest_groups[k + 1].clone()));
    }
    group_roles.push((nest_groups[nest_depth - 1].clone(), ROLE_ADMIN_ID.into()));

    let mut users = Vec::new();
    let mut user_memberships = Vec::new();
    let mut user_roles = Vec::new();
    let mut reports_to = Vec::new();
    let mut devices = Vec::new();
    let mut owns = Vec::new();

    for i in 0..n_users {
        let id = format!("user-{i}");
        users.push(ordinary_user(&id, i));
        let primary = i % n_groups;
        user_memberships.push((id.clone(), format!("grp-{primary}")));
        if rng.random_bool(0.35) {
            // n_groups >= 8 (clamped earlier), so the skip never wraps onto the primary.
            let skip = 1 + rng.random_range(0..n_groups - 1);
            let other = (primary + skip) % n_groups;
            user_memberships.push((id.clone(), format!("grp-{other}")));
        }
        if rng.random_bool(0.25) {
            user_roles.push(UserRoleGrant {
                user: id.clone(),
                role: ROLE_VIEWER_ID.into(),
                until: None,
            });
        }
        // Sparse tree: most users report up; every 9th ordinary user has no
        // manager (i > 0 keeps user-0 rootless, and i / 8 == i only at i == 0).
        if i > 0 && i % 9 != 0 {
            reports_to.push((id.clone(), format!("user-{}", i / 8)));
        }
        if rng.random_bool(0.20) {
            let did = format!("dev-{i}");
            devices.push(IdentityDevice { id: did.clone() });
            owns.push((id, did));
        }
    }

    let nest = plant_nest_fan(&nest_groups, group_k);
    users.extend(nest.users);
    user_memberships.extend(nest.user_memberships);
    reports_to.extend(nest.reports_to);

    let orphan = plant_orphan_priv(n_groups);
    users.push(orphan.user);
    user_roles.push(orphan.admin_grant);
    user_memberships.push(orphan.membership);

    let sod = plant_sod();
    users.push(sod.user);
    user_roles.extend([sod.requester, sod.approver]);
    user_memberships.push(sod.membership);
    reports_to.push(sod.report);

    let stale = plant_stale();
    users.push(stale.user);
    user_roles.push(stale.expired_grant);
    user_memberships.push(stale.membership);
    reports_to.push(stale.report);

    let IdentityCatalog {
        apps,
        roles,
        permissions,
        grants,
        ons,
    } = catalog();

    IdentityGraph {
        seed: cfg.seed,
        scale: cfg.scale,
        users,
        groups,
        roles,
        permissions,
        apps,
        devices,
        user_memberships,
        group_memberships,
        user_roles,
        group_roles,
        grants,
        ons,
        reports_to,
        owns,
        planted: IdentityPlanted {
            nested_admin: nest.nested_admin,
            nest_groups,
            fan_group: nest.fan_group,
            nest_depth: nest_depth as i64,
            group_k: group_k as i64,
            leaf_users: nest.leaf_users,
            orphan_priv: orphan.orphan_priv,
            sod_user: sod.sod_user,
            stale_grant: stale.stale_grant,
            stale_until: STALE_UNTIL.into(),
        },
    }
}

/// Rows planted by [`plant_nest_fan`]: the pattern's users, group
/// memberships, and reports to the org root, plus the ids
/// [`IdentityPlanted`] records. `leaf_users` runs admin-first, matching
/// the `users` row order.
struct NestFanRows {
    nested_admin: String,
    /// The nest group the fan joins: the first-group choice, made in
    /// [`plant_nest_fan`].
    fan_group: String,
    leaf_users: Vec<String>,
    users: Vec<IdentityUser>,
    user_memberships: Vec<(String, String)>,
    reports_to: Vec<(String, String)>,
}

/// Plants the deep-nesting pattern: the `nested-admin` user joins the first
/// nest group, and a fan of `nest-leaf` users joins the same group, each
/// reporting to the org root. Returns the pattern's rows, including the
/// anchor group the fan joins.
fn plant_nest_fan(nest_groups: &[String], group_k: usize) -> NestFanRows {
    let fan_group = nest_groups[0].clone();
    let mut users = Vec::new();
    let mut user_memberships = Vec::new();
    let mut reports_to = Vec::new();
    let mut leaf_users = Vec::new();
    let nested_admin = "nested-admin".to_string();
    users.push(planted_user(&nested_admin, "Nested Admin", "nested-admin"));
    user_memberships.push((nested_admin.clone(), fan_group.clone()));
    leaf_users.push(nested_admin.clone());
    for k in 0..group_k.saturating_sub(1) {
        let id = format!("leaf-{k}");
        users.push(planted_user(&id, &format!("Leaf {k}"), "nest-leaf"));
        user_memberships.push((id.clone(), fan_group.clone()));
        reports_to.push((id.clone(), ROOT_USER_ID.into()));
        leaf_users.push(id);
    }
    NestFanRows {
        nested_admin,
        fan_group,
        leaf_users,
        users,
        user_memberships,
        reports_to,
    }
}

/// Rows planted by [`plant_orphan_priv`]: the user, its admin grant, and
/// its membership in the last ordinary group. The pattern plants no
/// report — the orphan has no manager.
struct OrphanPrivRows {
    orphan_priv: String,
    user: IdentityUser,
    admin_grant: UserRoleGrant,
    membership: (String, String),
}

/// Plants `orphan-priv`: the admin role with no manager, a member of the
/// last ordinary group. Returns the pattern's rows.
fn plant_orphan_priv(n_groups: usize) -> OrphanPrivRows {
    let orphan_priv = "orphan-priv".to_string();
    OrphanPrivRows {
        orphan_priv: orphan_priv.clone(),
        user: planted_user(&orphan_priv, "Orphan Priv", "orphan-priv"),
        admin_grant: UserRoleGrant {
            user: orphan_priv.clone(),
            role: ROLE_ADMIN_ID.into(),
            until: None,
        },
        membership: (orphan_priv, format!("grp-{}", n_groups - 1)),
    }
}

/// Rows planted by [`plant_sod`]: the user, its conflicting requester and
/// approver grants, its `grp-0` membership, and its report to the org
/// root.
struct SodRows {
    sod_user: String,
    user: IdentityUser,
    requester: UserRoleGrant,
    approver: UserRoleGrant,
    membership: (String, String),
    report: (String, String),
}

/// Plants `sod`: a member of `grp-0` holding both the requester and the
/// approver role, reporting to the org root. Returns the pattern's rows,
/// the requester grant before the approver.
fn plant_sod() -> SodRows {
    let sod_user = "sod-user".to_string();
    SodRows {
        sod_user: sod_user.clone(),
        user: planted_user(&sod_user, "SoD User", "sod"),
        requester: UserRoleGrant {
            user: sod_user.clone(),
            role: ROLE_REQUESTER_ID.into(),
            until: None,
        },
        approver: UserRoleGrant {
            user: sod_user.clone(),
            role: ROLE_APPROVER_ID.into(),
            until: None,
        },
        membership: (sod_user.clone(), "grp-0".into()),
        report: (sod_user, ROOT_USER_ID.into()),
    }
}

/// Rows planted by [`plant_stale`]: the user, its editor grant expired on
/// `STALE_UNTIL`, its `grp-1` membership, and its report to the org root.
struct StaleRows {
    stale_grant: String,
    user: IdentityUser,
    expired_grant: UserRoleGrant,
    membership: (String, String),
    report: (String, String),
}

/// Plants `stale`: a member of `grp-1` holding an editor grant that expired
/// on `STALE_UNTIL`, reporting to the org root. Returns the pattern's
/// rows.
fn plant_stale() -> StaleRows {
    let stale_grant = "stale-grant".to_string();
    StaleRows {
        stale_grant: stale_grant.clone(),
        user: planted_user(&stale_grant, "Stale Grant", "stale"),
        expired_grant: UserRoleGrant {
            user: stale_grant.clone(),
            role: ROLE_EDITOR_ID.into(),
            until: Some(STALE_UNTIL.into()),
        },
        membership: (stale_grant.clone(), "grp-1".into()),
        report: (stale_grant, ROOT_USER_ID.into()),
    }
}

/// Input-free IAM fixture tables, shared by every [`generate`] run: the
/// app, role, and permission catalogs, the role→permission grants, and the
/// permission→app ons.
struct IdentityCatalog {
    apps: Vec<IdentityApp>,
    roles: Vec<IdentityRole>,
    permissions: Vec<IdentityPermission>,
    grants: Vec<(String, String)>,
    ons: Vec<(String, String)>,
}

/// Builds the IAM fixture tables; input-free, so every seed and scale
/// shares them.
fn catalog() -> IdentityCatalog {
    IdentityCatalog {
        apps: vec![
            app(APP_PAYROLL_ID, "Payroll"),
            app(APP_CRM_ID, "CRM"),
            app(APP_WIKI_ID, "Wiki"),
            app(APP_ADMIN_ID, "Admin Console"),
        ],
        roles: vec![
            role(ROLE_VIEWER_ID, "viewer"),
            role(ROLE_EDITOR_ID, "editor"),
            role(ROLE_ADMIN_ID, "admin"),
            role(ROLE_REQUESTER_ID, "requester"),
            role(ROLE_APPROVER_ID, "approver"),
        ],
        permissions: vec![
            perm(PERM_WIKI_READ_ID, "wiki.read"),
            perm(PERM_CRM_WRITE_ID, "crm.write"),
            perm(PERM_ADMIN_ALL_ID, "admin.all"),
            perm(PERM_PAYROLL_READ_ID, "payroll.read"),
        ],
        grants: vec![
            (ROLE_VIEWER_ID.into(), PERM_WIKI_READ_ID.into()),
            (ROLE_EDITOR_ID.into(), PERM_CRM_WRITE_ID.into()),
            (ROLE_ADMIN_ID.into(), PERM_ADMIN_ALL_ID.into()),
            (ROLE_REQUESTER_ID.into(), PERM_PAYROLL_READ_ID.into()),
            (ROLE_APPROVER_ID.into(), PERM_PAYROLL_READ_ID.into()),
        ],
        ons: vec![
            (PERM_WIKI_READ_ID.into(), APP_WIKI_ID.into()),
            (PERM_CRM_WRITE_ID.into(), APP_CRM_ID.into()),
            (PERM_ADMIN_ALL_ID.into(), APP_ADMIN_ID.into()),
            (PERM_PAYROLL_READ_ID.into(), APP_PAYROLL_ID.into()),
        ],
    }
}

fn ordinary_user(id: &str, i: usize) -> IdentityUser {
    let email = format!("{id}@example.com");
    let mut emails = vec![email.clone()];
    if i.is_multiple_of(5) {
        emails.push(format!("{id}.alias@example.com"));
    }
    IdentityUser {
        id: id.into(),
        name: format!("User {i}"),
        email,
        status: if i.is_multiple_of(17) {
            "inactive"
        } else {
            "active"
        }
        .into(),
        hired: hired_iso(i),
        emails,
        planted: None,
    }
}

fn planted_user(id: &str, name: &str, planted: &str) -> IdentityUser {
    let email = format!("{id}@example.com");
    IdentityUser {
        id: id.into(),
        name: name.into(),
        email: email.clone(),
        status: "active".into(),
        hired: "2019-03-15".into(),
        emails: vec![email],
        planted: Some(planted.into()),
    }
}

fn hired_iso(i: usize) -> String {
    let day = 1 + (i % 28) as u32;
    let month = 1 + (i % 12) as u32;
    let year = 2015 + (i % 10) as i32;
    format!("{year:04}-{month:02}-{day:02}")
}

fn app(id: &str, name: &str) -> IdentityApp {
    IdentityApp {
        id: id.into(),
        name: name.into(),
    }
}

fn role(id: &str, name: &str) -> IdentityRole {
    IdentityRole {
        id: id.into(),
        name: name.into(),
    }
}

fn perm(id: &str, name: &str) -> IdentityPermission {
    IdentityPermission {
        id: id.into(),
        name: name.into(),
    }
}
