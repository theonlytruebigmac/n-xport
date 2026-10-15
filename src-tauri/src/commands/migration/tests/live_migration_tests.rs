//! Live migration between two real servers. Ignored by default.
//!
//! The source is only ever read. The destination is written, so the test refuses to run
//! unless both service orgs have exactly the expected names. Needs NC_LIVE_SRC_URL,
//! NC_LIVE_SRC_JWT_FILE, NC_LIVE_SRC_SO_ID, NC_LIVE_SRC_SO_NAME, NC_LIVE_DST_URL,
//! NC_LIVE_DST_JWT_FILE, NC_LIVE_DST_SO_ID, NC_LIVE_DST_SO_NAME and NC_LIVE_DST_API_USERNAME.

use crate::api::client::NcClient;
use crate::api::NcSoapClient;
use crate::commands::migration::env::MigrationEnv;
use crate::commands::migration::run::run_migration;
use crate::commands::migration::types::{MigrationOptions, MigrationResult};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::test::mock_app;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {}", name))
}

async fn connect(prefix: &str) -> (NcClient, String, String, i64) {
    let jwt = std::fs::read_to_string(env(&format!("{}_JWT_FILE", prefix)))
        .expect("jwt file")
        .trim()
        .to_string();
    let url = env(&format!("{}_URL", prefix));
    let so_id: i64 = env(&format!("{}_SO_ID", prefix))
        .parse()
        .expect("numeric SO id");
    let expected = env(&format!("{}_SO_NAME", prefix));
    let client = NcClient::new(&url);
    client.authenticate(&jwt).await.expect("authenticate");
    let so = client
        .get_service_org_by_id(so_id)
        .await
        .expect("service org");
    assert_eq!(
        so.so_name, expected,
        "refusing to continue: service org {} on {} is '{}', not '{}'",
        so_id, url, so.so_name, expected
    );
    (client, url, jwt, so_id)
}

/// Everything a migration writes, as a comparable snapshot of one service org.
#[derive(Debug, PartialEq)]
struct Snapshot {
    customers: BTreeSet<String>,
    sites: BTreeSet<String>,
    users: BTreeSet<String>,
    roles: BTreeSet<String>,
    groups: BTreeSet<String>,
}

async fn snapshot(client: &NcClient, so_id: i64) -> Snapshot {
    let customers = client.get_customers_by_so(so_id).await.expect("customers");
    let customer_ids: std::collections::HashSet<i64> =
        customers.iter().map(|c| c.customer_id).collect();
    let sites = client.get_sites_by_so(so_id).await.expect("sites");
    let mut snap = Snapshot {
        customers: customers.iter().map(|c| c.customer_name.clone()).collect(),
        sites: sites
            .iter()
            .filter(|s| {
                s.parent_id
                    .or(s.customer_id)
                    .or(s.customerid)
                    .is_some_and(|p| customer_ids.contains(&p))
            })
            .map(|s| s.site_name.clone())
            .collect(),
        users: BTreeSet::new(),
        roles: BTreeSet::new(),
        groups: BTreeSet::new(),
    };
    let mut ous = vec![so_id];
    ous.extend(customer_ids.iter().copied());
    for ou in ous {
        snap.users.extend(
            client
                .get_users_by_org_unit(ou)
                .await
                .expect("users")
                .into_iter()
                .map(|u| u.login_name),
        );
        snap.roles.extend(
            client
                .get_user_roles(ou)
                .await
                .expect("roles")
                .into_iter()
                .filter_map(|r| r.role_name),
        );
        snap.groups.extend(
            client
                .get_access_groups(ou)
                .await
                .expect("groups")
                .into_iter()
                .filter_map(|g| g.group_name),
        );
    }
    snap
}

fn show(label: &str, result: &MigrationResult) {
    println!("--- {} ---", label);
    println!(
        "success={} cancelled={} message={}",
        result.success, result.cancelled, result.message
    );
    println!("issues ({}):", result.issues.len());
    for item in &result.issues {
        println!("  ! {}", item);
    }
    println!("skipped: {}", result.skipped.len());
    println!("manual follow-up ({}):", result.manual_follow_up.len());
    for item in &result.manual_follow_up {
        println!("  > {}", item);
    }
}

#[tokio::test]
#[ignore = "writes to a live server"]
async fn full_migration_between_two_live_servers_then_a_repeat() {
    let (source, _, _, source_so) = connect("NC_LIVE_SRC").await;
    let (dest, dest_url, dest_jwt, dest_so) = connect("NC_LIVE_DST").await;
    let mut soap = NcSoapClient::new(&dest_url, &dest_jwt);
    soap.set_username(&env("NC_LIVE_DST_API_USERNAME"));

    // NC_LIVE_MIGRATE_ONLY (comma separated, e.g. "users") limits which steps run.
    let only = std::env::var("NC_LIVE_MIGRATE_ONLY").ok();
    let selected = |name: &str| {
        only.as_ref()
            .is_none_or(|list| list.split(',').any(|item| item.trim() == name))
    };
    let options = MigrationOptions {
        customers: selected("customers"),
        user_roles: selected("roles"),
        access_groups: selected("groups"),
        users: selected("users"),
        org_properties: selected("properties"),
        device_properties: selected("devices"),
    };
    let preflight =
        crate::commands::migration_preflight::build_preflight(&options, source_so, dest_so);
    println!(
        "preflight steps: {:?}",
        preflight
            .steps
            .iter()
            .map(|s| (&s.resource, s.soap))
            .collect::<Vec<_>>()
    );

    let app = mock_app();
    let cancel = Arc::new(AtomicBool::new(false));
    let env = MigrationEnv {
        source: &source,
        dest: &dest,
        source_so_id: source_so,
        dest_so_id: dest_so,
        soap: Some(&soap),
        app_handle: app.handle(),
        cancel_token: &cancel,
    };

    let source_before = snapshot(&source, source_so).await;
    let dest_before = snapshot(&dest, dest_so).await;
    println!(
        "before: source {{customers {}, sites {}, users {}, roles {}, groups {}}} dest {{customers {}, sites {}, users {}, roles {}, groups {}}}",
        source_before.customers.len(), source_before.sites.len(), source_before.users.len(), source_before.roles.len(), source_before.groups.len(),
        dest_before.customers.len(), dest_before.sites.len(), dest_before.users.len(), dest_before.roles.len(), dest_before.groups.len(),
    );

    let first = run_migration(&env, &options)
        .await
        .expect("first migration");
    show("first migration", &first);
    let dest_after_first = snapshot(&dest, dest_so).await;
    println!(
        "added by first run: users {:?}, roles {:?}, groups {:?}",
        dest_after_first
            .users
            .difference(&dest_before.users)
            .collect::<Vec<_>>(),
        dest_after_first
            .roles
            .difference(&dest_before.roles)
            .collect::<Vec<_>>(),
        dest_after_first
            .groups
            .difference(&dest_before.groups)
            .collect::<Vec<_>>(),
    );
    println!(
        "still missing on the destination: users {:?}, roles {:?}, groups {:?}",
        source_before
            .users
            .difference(&dest_after_first.users)
            .collect::<Vec<_>>(),
        source_before
            .roles
            .difference(&dest_after_first.roles)
            .collect::<Vec<_>>(),
        source_before
            .groups
            .difference(&dest_after_first.groups)
            .collect::<Vec<_>>(),
    );

    let second = run_migration(&env, &options)
        .await
        .expect("second migration");
    show("repeat migration", &second);
    let dest_after_second = snapshot(&dest, dest_so).await;
    assert_eq!(
        dest_after_first, dest_after_second,
        "a repeat migration must create nothing"
    );
    assert_eq!(
        source_before,
        snapshot(&source, source_so).await,
        "the source must not change"
    );
}
