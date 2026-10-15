//! Dry-run import rows built from a live server's own data. Ignored by default; needs
//! NC_LIVE_BASE_URL, NC_LIVE_JWT_FILE, NC_LIVE_SO_ID. Every call is a dry run, so only GET
//! requests are made and nothing on the server changes.

use crate::api::NcClient;
use crate::import::handlers::access_groups::import_access_group;
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::customers::import_customer;
use crate::import::handlers::properties::{import_device_property, import_org_property};
use crate::import::handlers::roles::import_user_role;
use crate::import::handlers::sites::import_site;
use crate::import::handlers::users::import_user;
use crate::import::RowStatus;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use tauri::test::mock_app;

#[tokio::test]
#[ignore = "needs a live server"]
async fn dry_run_rows_resolve_against_live_data() {
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {}", name));
    let jwt = std::fs::read_to_string(env("NC_LIVE_JWT_FILE")).expect("jwt file");
    let so_id: i64 = env("NC_LIVE_SO_ID").parse().expect("numeric SO id");
    let client = NcClient::new(&env("NC_LIVE_BASE_URL"));
    client.authenticate(jwt.trim()).await.expect("authenticate");
    let app = mock_app();
    let cancel = AtomicBool::new(false);
    let mut ctx = ImportContext::load(&client, so_id, &cancel)
        .await
        .expect("context");
    println!(
        "context: {} customers, {} sites, {} roles at SO, {} groups at SO",
        ctx.customers_by_name.len(),
        ctx.sites_by_name.len(),
        ctx.roles_by_ou.get(&so_id).map_or(0, |m| m.len()),
        ctx.groups_by_ou.get(&so_id).map_or(0, |m| m.len())
    );

    let customers = client.get_customers_by_so(so_id).await.expect("customers");
    let customer = customers
        .iter()
        .find(|c| {
            ctx.sites_by_name
                .keys()
                .any(|(n, _)| *n == c.customer_name.to_lowercase())
        })
        .or(customers.first())
        .expect("a customer");
    let (site_customer, site_name) = ctx.sites_by_name.keys().next().cloned().expect("a site");
    let role = ctx.roles_by_ou[&so_id]
        .keys()
        .next()
        .cloned()
        .expect("a role");
    let group = ctx.groups_by_ou[&so_id]
        .keys()
        .next()
        .cloned()
        .expect("a group");

    let mut outcomes = Vec::new();
    outcomes.push((
        "existing customer",
        import_customer(
            2,
            serde_json::from_value(json!({"customerName": customer.customer_name})).unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "new customer",
        import_customer(
            3,
            serde_json::from_value(json!({"customerName": "Nxport Live Test (not created)"}))
                .unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "existing site",
        import_site(
            4,
            serde_json::from_value(json!({"customerName": site_customer, "siteName": site_name}))
                .unwrap(),
            &mut ctx,
            &client,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push(("new site", import_site(5, serde_json::from_value(json!({"customerName": customer.customer_name, "siteName": "Nxport Live Test Site"})).unwrap(), &mut ctx, &client, true, app.handle()).await));
    outcomes.push((
        "existing role at SO",
        import_user_role(
            6,
            serde_json::from_value(json!({"roleName": role})).unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "inherited role at customer",
        import_user_role(
            7,
            serde_json::from_value(
                json!({"roleName": role, "customerName": customer.customer_name}),
            )
            .unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "new role",
        import_user_role(
            8,
            serde_json::from_value(json!({"roleName": "Nxport Live Test Role"})).unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "existing group",
        import_access_group(
            9,
            serde_json::from_value(json!({"groupName": group})).unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push((
        "new group",
        import_access_group(
            10,
            serde_json::from_value(json!({"groupName": "Nxport Live Test Group"})).unwrap(),
            &mut ctx,
            &client,
            None,
            true,
            app.handle(),
        )
        .await,
    ));
    outcomes.push(("user at SO", import_user(11, serde_json::from_value(json!({"email": "nxport.live.test@example.invalid", "firstName": "Live", "lastName": "Test", "customerName": "SO", "roleNames": role})).unwrap(), &mut ctx, None, true, None, app.handle()).await));

    let props = client.get_org_properties(so_id).await.expect("org props");
    if let Some(prop) = props
        .iter()
        .find(|p| p.label.is_some() && p.property_type.as_deref() != Some("ENUMERATED"))
    {
        let label = prop.label.clone().unwrap();
        outcomes.push((
            "org property",
            import_org_property(
                12,
                serde_json::from_value(json!({"propertyName": label, "value": "x"})).unwrap(),
                &mut ctx,
                &client,
                true,
                &cancel,
                app.handle(),
            )
            .await,
        ));
    }
    outcomes.push((
        "unknown org property",
        import_org_property(
            13,
            serde_json::from_value(json!({"propertyName": "No Such Property", "value": "x"}))
                .unwrap(),
            &mut ctx,
            &client,
            true,
            &cancel,
            app.handle(),
        )
        .await,
    ));

    // A device under a customer, with one of its own properties.
    'devices: for c in &customers {
        for d in client
            .get_devices_by_org_unit(c.customer_id)
            .await
            .expect("devices")
        {
            if let (Some(name), true) = (
                d.long_name.clone(),
                d.org_unit_scope_id() == Some(c.customer_id),
            ) {
                let dprops = client
                    .get_device_properties(d.device_id)
                    .await
                    .expect("device props");
                if let Some(label) = dprops
                    .iter()
                    .find(|p| p.label.is_some() && p.property_type.as_deref() != Some("ENUMERATED"))
                    .and_then(|p| p.label.clone())
                {
                    outcomes.push(("device property", import_device_property(14, serde_json::from_value(json!({"customerName": c.customer_name, "deviceName": name, "propertyName": label, "value": "x"})).unwrap(), &mut ctx, &client, true, &cancel, app.handle()).await));
                    break 'devices;
                }
            }
        }
    }

    for (what, outcome) in &outcomes {
        println!("{:<28} {:?}: {}", what, outcome.status, outcome.message);
    }
    let status = |what: &str| {
        outcomes
            .iter()
            .find(|(w, _)| *w == what)
            .map(|(_, o)| o.status)
    };
    assert_eq!(status("existing customer"), Some(RowStatus::Skipped));
    assert_eq!(status("new customer"), Some(RowStatus::Planned));
    assert_eq!(status("existing site"), Some(RowStatus::Skipped));
    assert_eq!(status("new site"), Some(RowStatus::Planned));
    assert_eq!(status("existing role at SO"), Some(RowStatus::Skipped));
    assert_eq!(
        status("inherited role at customer"),
        Some(RowStatus::Skipped)
    );
    assert_eq!(status("new role"), Some(RowStatus::Planned));
    assert_eq!(status("existing group"), Some(RowStatus::Skipped));
    assert_eq!(status("new group"), Some(RowStatus::Planned));
    assert_eq!(status("user at SO"), Some(RowStatus::Planned));
    assert_eq!(status("unknown org property"), Some(RowStatus::Error));
    if let Some(s) = status("org property") {
        assert_eq!(s, RowStatus::Planned);
    }
    if let Some(s) = status("device property") {
        assert_eq!(s, RowStatus::Planned);
    }
}
