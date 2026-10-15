//! Read-only checks of the API client and models against a real N-central server.
//!
//! Ignored by default. Run with:
//!
//! ```text
//! NC_LIVE_BASE_URL=https://nc.example.com \
//! NC_LIVE_JWT_FILE=/path/to/jwt \
//! NC_LIVE_SO_ID=50 \
//! cargo test --test live_readonly -- --ignored --nocapture
//! ```
//!
//! Only GET requests are made (no create or update client methods are called), so this
//! is safe to point at a server you do not want to change. The JWT is read from a file
//! and never printed.

use nc_data_export_lib::api::NcClient;
use std::collections::HashMap;
use std::fmt::Display;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {} to run the live tests", name))
}

struct Report {
    errors: Vec<String>,
}

impl Report {
    /// Record one call's outcome and return its value if it succeeded.
    fn check<T, E: Display>(
        &mut self,
        what: &str,
        result: Result<T, E>,
        count: impl Fn(&T) -> usize,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                println!("ok    {:<48} {} item(s)", what, count(&value));
                Some(value)
            }
            Err(e) => {
                println!("ERR   {:<48} {}", what, e);
                self.errors.push(format!("{}: {}", what, e));
                None
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs a live server; see the module docs"]
async fn read_only_endpoints_parse_against_the_live_server() {
    let base_url = env("NC_LIVE_BASE_URL");
    let jwt = std::fs::read_to_string(env("NC_LIVE_JWT_FILE")).expect("read JWT file");
    let so_id: i64 = env("NC_LIVE_SO_ID")
        .parse()
        .expect("NC_LIVE_SO_ID must be a number");

    let client = NcClient::new(&base_url);
    client.authenticate(jwt.trim()).await.expect("authenticate");
    let mut report = Report { errors: Vec::new() };

    let info = client.get_server_info().await.expect("server info");
    println!("server: {:?}", info);

    report.check("service orgs", client.get_service_orgs().await, Vec::len);
    report.check(
        "service org by id",
        client.get_service_org_by_id(so_id).await.map(|so| vec![so]),
        Vec::len,
    );
    let customers = report
        .check(
            "customers of SO",
            client.get_customers_by_so(so_id).await,
            Vec::len,
        )
        .unwrap_or_default();
    let sites = report
        .check(
            "sites (global list)",
            client.get_sites_by_so(so_id).await,
            Vec::len,
        )
        .unwrap_or_default();

    // Org units to inspect: the SO, each customer, and a few sites.
    let mut org_units: Vec<(String, i64)> = vec![(format!("SO {}", so_id), so_id)];
    org_units.extend(
        customers
            .iter()
            .take(4)
            .map(|c| (format!("customer {}", c.customer_id), c.customer_id)),
    );
    for customer in customers.iter().take(3) {
        let by_customer = report.check(
            &format!("sites of customer {}", customer.customer_id),
            client.get_sites_by_customer(customer.customer_id).await,
            Vec::len,
        );
        if let Some(by_customer) = by_customer {
            org_units.extend(
                by_customer
                    .iter()
                    .take(1)
                    .map(|s| (format!("site {}", s.site_id), s.site_id)),
            );
        }
    }
    println!("global site list has {} site(s)", sites.len());

    let mut roles_seen: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
    let mut groups_seen: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
    let mut first_device = None;
    for (label, ou) in &org_units {
        report.check(
            &format!("users at {}", label),
            client.get_users_by_org_unit(*ou).await,
            Vec::len,
        );
        if let Some(roles) = report.check(
            &format!("roles at {}", label),
            client.get_user_roles(*ou).await,
            Vec::len,
        ) {
            roles_seen.insert(
                *ou,
                roles
                    .iter()
                    .filter_map(|r| r.role_name.clone().map(|n| (n, r.role_id)))
                    .collect(),
            );
        }
        if let Some(groups) = report.check(
            &format!("access groups at {}", label),
            client.get_access_groups(*ou).await,
            Vec::len,
        ) {
            groups_seen.insert(
                *ou,
                groups
                    .iter()
                    .filter_map(|g| g.group_name.clone().map(|n| (n, g.group_id)))
                    .collect(),
            );
        }
        report.check(
            &format!("org properties at {}", label),
            client.get_org_properties(*ou).await,
            Vec::len,
        );
        if let Some(devices) = report.check(
            &format!("devices at {}", label),
            client.get_devices_by_org_unit(*ou).await,
            Vec::len,
        ) {
            if first_device.is_none() {
                first_device = devices.first().map(|d| d.device_id);
            }
        }
    }
    if let Some(device_id) = first_device {
        report.check(
            &format!("device properties of {}", device_id),
            client.get_device_properties(device_id).await,
            Vec::len,
        );
        report.check(
            &format!("device assets of {}", device_id),
            client.get_device_assets(device_id).await.map(|a| vec![a]),
            Vec::len,
        );
    }

    // Inheritance: do roles/groups defined at the SO show up at its customers, and with which IDs?
    for (what, seen) in [("role", &roles_seen), ("access group", &groups_seen)] {
        let Some(at_so) = seen.get(&so_id) else {
            continue;
        };
        for customer in customers.iter().take(4) {
            let Some(at_customer) = seen.get(&customer.customer_id) else {
                continue;
            };
            let by_name: HashMap<&str, i64> = at_customer
                .iter()
                .map(|(n, id)| (n.as_str(), *id))
                .collect();
            let inherited = at_so
                .iter()
                .filter(|(n, _)| by_name.contains_key(n.as_str()))
                .count();
            let same_id = at_so
                .iter()
                .filter(|(n, id)| by_name.get(n.as_str()) == Some(id))
                .count();
            println!(
                "inheritance: {} SO {} -> customer {}: {}/{} SO {}s visible at the customer, {} with the same ID",
                what, so_id, customer.customer_id, inherited, at_so.len(), what, same_id
            );
        }
    }

    assert!(
        report.errors.is_empty(),
        "calls that failed against the live server:\n{}",
        report.errors.join("\n")
    );
}
