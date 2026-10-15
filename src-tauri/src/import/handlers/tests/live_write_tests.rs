//! Live WRITE tests. Ignored by default. They create real objects (the REST API has no
//! delete), so they refuse to run unless the target service org is the one named in
//! `NC_LIVE_WRITE_SO_NAME` and that name matches the server's own record for
//! `NC_LIVE_WRITE_SO_ID`. Every write goes through the import handlers with a context loaded
//! for that one service org, so it cannot reach another service org.
//!
//! Needs NC_LIVE_BASE_URL, NC_LIVE_JWT_FILE, NC_LIVE_WRITE_SO_ID, NC_LIVE_WRITE_SO_NAME and,
//! for the SOAP steps, NC_LIVE_API_USERNAME.

use crate::api::{NcClient, NcSoapClient};
use crate::import::handlers::access_groups::import_access_group;
use crate::import::handlers::context::ImportContext;
use crate::import::handlers::customers::import_customer;
use crate::import::handlers::properties::{import_device_property, import_org_property};
use crate::import::handlers::roles::import_user_role;
use crate::import::handlers::sites::import_site;
use crate::import::handlers::users::import_user;
use crate::import::{RowOutcome, RowStatus};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use tauri::test::mock_app;

const TEST_CUSTOMER: &str = "nxport-live-test-customer";
const TEST_SITE: &str = "nxport-live-test-site";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {}", name))
}

struct Live {
    client: NcClient,
    soap: NcSoapClient,
    so_id: i64,
}

/// Connect, and refuse to continue unless the target is exactly the allowed service org.
async fn connect() -> Live {
    let jwt = std::fs::read_to_string(env("NC_LIVE_JWT_FILE")).expect("jwt file");
    let jwt = jwt.trim().to_string();
    let base_url = env("NC_LIVE_BASE_URL");
    let so_id: i64 = env("NC_LIVE_WRITE_SO_ID").parse().expect("numeric SO id");
    let allowed_name = env("NC_LIVE_WRITE_SO_NAME");
    let client = NcClient::new(&base_url);
    client.authenticate(&jwt).await.expect("authenticate");
    let so = client
        .get_service_org_by_id(so_id)
        .await
        .expect("target service org");
    assert_eq!(
        so.so_name, allowed_name,
        "refusing to write: service org {} is '{}', not '{}'",
        so_id, so.so_name, allowed_name
    );
    let mut soap = NcSoapClient::new(&base_url, &jwt);
    if let Ok(user) = std::env::var("NC_LIVE_API_USERNAME") {
        soap.set_username(&user);
    }
    Live {
        client,
        soap,
        so_id,
    }
}

async fn context(live: &Live) -> ImportContext {
    ImportContext::load(&live.client, live.so_id, &AtomicBool::new(false))
        .await
        .expect("context for the target service org")
}

fn show(what: &str, outcome: &RowOutcome) {
    println!("{:<34} {:?}: {}", what, outcome.status, outcome.message);
}

#[tokio::test]
#[ignore = "writes to a live server"]
async fn customer_and_site_are_created_once_and_skipped_on_repeat() {
    let live = connect().await;
    let app = mock_app();

    let mut ctx = context(&live).await;
    let first = import_customer(
        2,
        serde_json::from_value(json!({"customerName": TEST_CUSTOMER})).unwrap(),
        &mut ctx,
        &live.client,
        None,
        false,
        app.handle(),
    )
    .await;
    show("customer (first run)", &first);
    assert!(
        matches!(first.status, RowStatus::Created | RowStatus::Skipped),
        "{}",
        first.message
    );
    if first.status == RowStatus::Created {
        assert!(
            !first.message.contains("no ID"),
            "live create response carried no ID: {}",
            first.message
        );
    }

    // A fresh context is what the next app session would see.
    let mut ctx = context(&live).await;
    let again = import_customer(
        3,
        serde_json::from_value(json!({"customerName": TEST_CUSTOMER})).unwrap(),
        &mut ctx,
        &live.client,
        None,
        false,
        app.handle(),
    )
    .await;
    show("customer (repeat)", &again);
    assert_eq!(again.status, RowStatus::Skipped);

    let customers = live
        .client
        .get_customers_by_so(live.so_id)
        .await
        .expect("customers");
    let created = customers
        .iter()
        .filter(|c| c.customer_name == TEST_CUSTOMER)
        .count();
    assert_eq!(
        created, 1,
        "exactly one test customer must exist under the service org"
    );

    let site = import_site(
        4,
        serde_json::from_value(json!({"customerName": TEST_CUSTOMER, "siteName": TEST_SITE}))
            .unwrap(),
        &mut ctx,
        &live.client,
        false,
        app.handle(),
    )
    .await;
    show("site (first run)", &site);
    assert!(
        matches!(site.status, RowStatus::Created | RowStatus::Skipped),
        "{}",
        site.message
    );
    let mut ctx = context(&live).await;
    let site_again = import_site(
        5,
        serde_json::from_value(json!({"customerName": TEST_CUSTOMER, "siteName": TEST_SITE}))
            .unwrap(),
        &mut ctx,
        &live.client,
        false,
        app.handle(),
    )
    .await;
    show("site (repeat)", &site_again);
    assert_eq!(site_again.status, RowStatus::Skipped);
}

/// Set a property value, read it back from the server, then restore the original value.
#[tokio::test]
#[ignore = "writes to a live server"]
async fn organization_property_value_round_trips_and_blank_clears() {
    let live = connect().await;
    let app = mock_app();
    let cancel = AtomicBool::new(false);
    let mut ctx = context(&live).await;
    let customer_id = *ctx
        .customers_by_name
        .get(TEST_CUSTOMER)
        .expect("run the customer test first so the test customer exists");
    const PROPERTY: &str = "Migration Destination CustomerId";

    let read = |label: &'static str| {
        let client = live.client.clone();
        async move {
            let props = client
                .get_org_properties(customer_id)
                .await
                .expect("properties");
            let prop = props
                .into_iter()
                .find(|p| p.label.as_deref() == Some(PROPERTY))
                .unwrap_or_else(|| panic!("{}: property not listed", label));
            prop.value
        }
    };
    let before = read("before").await;
    println!("value before: {:?}", before);

    let set = import_org_property(2, serde_json::from_value(json!({"customerName": TEST_CUSTOMER, "propertyName": PROPERTY, "value": "nxport-live-test"})).unwrap(), &mut ctx, &live.client, false, &cancel, app.handle()).await;
    show("set value", &set);
    assert_eq!(set.status, RowStatus::Updated, "{}", set.message);
    let after_set = read("after set").await;
    println!("value after set: {:?}", after_set);
    assert_eq!(after_set.as_deref(), Some("nxport-live-test"));

    let repeat = import_org_property(3, serde_json::from_value(json!({"customerName": TEST_CUSTOMER, "propertyName": PROPERTY, "value": "nxport-live-test"})).unwrap(), &mut ctx, &live.client, false, &cancel, app.handle()).await;
    show("set same value again", &repeat);
    assert_eq!(
        repeat.status,
        RowStatus::Updated,
        "a repeat write is a safe no-op"
    );

    let cleared = import_org_property(
        4,
        serde_json::from_value(
            json!({"customerName": TEST_CUSTOMER, "propertyName": PROPERTY, "value": ""}),
        )
        .unwrap(),
        &mut ctx,
        &live.client,
        false,
        &cancel,
        app.handle(),
    )
    .await;
    show("blank value", &cleared);
    assert_eq!(cleared.status, RowStatus::Updated, "{}", cleared.message);
    let after_clear = read("after clear").await;
    println!("value after clear: {:?}", after_clear);
    assert!(
        after_clear.as_deref().unwrap_or("").is_empty(),
        "a blank value must clear the property"
    );
}

/// The one device in the service org: set a marker on an empty TEXT property, then restore it.
#[tokio::test]
#[ignore = "writes to a live server"]
async fn device_property_value_round_trips() {
    let live = connect().await;
    let app = mock_app();
    let cancel = AtomicBool::new(false);
    let mut ctx = context(&live).await;
    const CUSTOMER: &str = "Test Client";
    const DEVICE: &str = "ZACHFRAZIER0B89";
    const PROPERTY: &str = "Peripherals";

    let customer_id = *ctx
        .customers_by_name
        .get("test client")
        .expect("customer Test Client");
    let devices = live
        .client
        .get_devices_by_org_unit(customer_id)
        .await
        .expect("devices");
    let device_id = devices
        .iter()
        .find(|d| d.long_name.as_deref() == Some(DEVICE))
        .map(|d| d.device_id)
        .expect("device");
    let read = || {
        let client = live.client.clone();
        async move {
            client
                .get_device_properties(device_id)
                .await
                .expect("device properties")
                .into_iter()
                .find(|p| p.label.as_deref() == Some(PROPERTY))
                .expect("property listed")
                .value
        }
    };
    let original = read().await;
    println!("original value: {:?}", original);
    assert!(
        original.as_deref().unwrap_or("").is_empty(),
        "refusing to overwrite a non-empty value"
    );

    let row = |value: &str| {
        serde_json::from_value(json!({"customerName": CUSTOMER, "deviceName": DEVICE, "propertyName": PROPERTY, "value": value})).unwrap()
    };
    let set = import_device_property(
        2,
        row("nxport-live-test"),
        &mut ctx,
        &live.client,
        false,
        &cancel,
        app.handle(),
    )
    .await;
    show("set device property", &set);
    let set_ok = set.status == RowStatus::Updated;
    let after_set = read().await;
    println!("value after set: {:?}", after_set);

    // Always restore, even if the assertions below fail.
    let restore = import_device_property(
        3,
        row(""),
        &mut ctx,
        &live.client,
        false,
        &cancel,
        app.handle(),
    )
    .await;
    show("restore (blank)", &restore);
    let restored = read().await;
    println!("value after restore: {:?}", restored);

    assert!(set_ok, "{}", set.message);
    assert_eq!(after_set.as_deref(), Some("nxport-live-test"));
    assert_eq!(restore.status, RowStatus::Updated, "{}", restore.message);
    assert!(
        restored.as_deref().unwrap_or("").is_empty(),
        "the original blank value must be restored"
    );
}

/// Where does a role created for a customer end up? Creates one role through REST and one
/// through SOAP (permanent: roles cannot be deleted through the API) and reports which
/// org units list each.
#[tokio::test]
#[ignore = "writes to a live server"]
async fn customer_level_role_placement_rest_versus_soap() {
    let live = connect().await;
    let app = mock_app();
    let mut ctx = context(&live).await;
    let customer_id = *ctx
        .customers_by_name
        .get(TEST_CUSTOMER)
        .expect("run the customer test first");
    let sibling_id = *ctx
        .customers_by_name
        .get("test client")
        .expect("sibling customer");

    let role_row = |name: &str| {
        serde_json::from_value(json!({"roleName": name, "customerName": TEST_CUSTOMER})).unwrap()
    };

    let rest = import_user_role(
        2,
        role_row("nxport-live-role-rest"),
        &mut ctx,
        &live.client,
        None,
        false,
        app.handle(),
    )
    .await;
    show("role via REST (no SOAP client)", &rest);
    let soap = import_user_role(
        3,
        role_row("nxport-live-role-soap"),
        &mut ctx,
        &live.client,
        Some(&live.soap),
        false,
        app.handle(),
    )
    .await;
    show("role via SOAP-first", &soap);

    async fn visible(client: &NcClient, ou: i64, name: &str) -> Option<i64> {
        client
            .get_user_roles(ou)
            .await
            .expect("roles")
            .into_iter()
            .find(|r| r.role_name.as_deref() == Some(name))
            .map(|r| r.role_id)
    }
    for name in ["nxport-live-role-rest", "nxport-live-role-soap"] {
        let at_customer = visible(&live.client, customer_id, name).await;
        let at_so = visible(&live.client, live.so_id, name).await;
        let at_sibling = visible(&live.client, sibling_id, name).await;
        println!(
            "{:<24} customer {}: {:?} | service org {}: {:?} | sibling customer {}: {:?}",
            name, customer_id, at_customer, live.so_id, at_so, sibling_id, at_sibling
        );
    }
}

/// Create an access group (REST) and a user (SOAP) for the test customer, then repeat both
/// from a fresh context to see how the live server answers duplicates.
#[tokio::test]
#[ignore = "writes to a live server"]
async fn access_group_and_user_are_created_once_and_skipped_on_repeat() {
    let live = connect().await;
    let app = mock_app();
    let mut ctx = context(&live).await;
    let customer_id = *ctx
        .customers_by_name
        .get(TEST_CUSTOMER)
        .expect("run the customer test first");
    const GROUP: &str = "nxport-live-group";
    const EMAIL: &str = "nxport.live.test@example.invalid";

    let group_row = || {
        serde_json::from_value(json!({"groupName": GROUP, "customerName": TEST_CUSTOMER, "orgUnitNames": TEST_CUSTOMER})).unwrap()
    };
    let group = import_access_group(
        2,
        group_row(),
        &mut ctx,
        &live.client,
        None,
        false,
        app.handle(),
    )
    .await;
    show("access group via REST", &group);
    assert!(
        matches!(group.status, RowStatus::Created | RowStatus::Skipped),
        "{}",
        group.message
    );
    let groups = live
        .client
        .get_access_groups(customer_id)
        .await
        .expect("groups");
    let listed = groups
        .iter()
        .find(|g| g.group_name.as_deref() == Some(GROUP))
        .expect("group listed at the customer");
    println!(
        "group {} org units {:?} auto-include {:?}",
        listed.group_id,
        listed.get_org_unit_ids(),
        listed.get_auto_include()
    );
    let at_so = live
        .client
        .get_access_groups(live.so_id)
        .await
        .expect("groups at SO");
    println!(
        "group also listed at the service org: {}",
        at_so.iter().any(|g| g.group_name.as_deref() == Some(GROUP))
    );

    let mut ctx = context(&live).await;
    let again = import_access_group(
        3,
        group_row(),
        &mut ctx,
        &live.client,
        None,
        false,
        app.handle(),
    )
    .await;
    show("access group repeat (new context)", &again);
    assert_eq!(again.status, RowStatus::Skipped);

    // The user is created through SOAP and given the SOAP-created role and the group.
    let user_row = || {
        serde_json::from_value(json!({
        "email": EMAIL, "firstName": "Nxport", "lastName": "LiveTest",
        "customerName": TEST_CUSTOMER, "roleNames": "nxport-live-role-soap", "accessGroupNames": GROUP,
    })).unwrap()
    };
    let user = import_user(
        4,
        user_row(),
        &mut ctx,
        Some(&live.soap),
        false,
        None,
        app.handle(),
    )
    .await;
    show("user via SOAP", &user);
    let users = live
        .client
        .get_users_by_org_unit(customer_id)
        .await
        .expect("users");
    let at_customer = users.iter().find(|u| u.login_name == EMAIL);
    println!(
        "user listed at the customer: {:?}",
        at_customer.map(|u| (u.user_id, u.role_ids.clone(), u.access_group_ids.clone()))
    );
    let so_users = live
        .client
        .get_users_by_org_unit(live.so_id)
        .await
        .expect("users at SO");
    println!(
        "user listed at the service org: {}",
        so_users.iter().any(|u| u.login_name == EMAIL)
    );

    let same_session = import_user(
        5,
        user_row(),
        &mut ctx,
        Some(&live.soap),
        false,
        None,
        app.handle(),
    )
    .await;
    show("user repeat (same session)", &same_session);
    let mut ctx = context(&live).await;
    let new_session = import_user(
        6,
        user_row(),
        &mut ctx,
        Some(&live.soap),
        false,
        None,
        app.handle(),
    )
    .await;
    show("user repeat (new context)", &new_session);
    assert_eq!(same_session.status, RowStatus::Skipped);
    assert_eq!(
        new_session.status,
        RowStatus::Skipped,
        "{}",
        new_session.message
    );
}

/// The live server answers a group creation with no body; the handler must still report
/// the real ID, and the repeat must then be a plain "already exists".
#[tokio::test]
#[ignore = "writes to a live server"]
async fn access_group_id_is_resolved_after_a_bodyless_create() {
    let live = connect().await;
    let app = mock_app();
    let mut ctx = context(&live).await;
    let customer_id = *ctx
        .customers_by_name
        .get(TEST_CUSTOMER)
        .expect("run the customer test first");
    const GROUP: &str = "nxport-live-group-id-check";
    let row = || {
        serde_json::from_value(json!({"groupName": GROUP, "customerName": TEST_CUSTOMER, "orgUnitNames": TEST_CUSTOMER})).unwrap()
    };

    let first =
        import_access_group(2, row(), &mut ctx, &live.client, None, false, app.handle()).await;
    show("group create", &first);
    let listed = live
        .client
        .find_access_group_id(customer_id, GROUP)
        .await
        .expect("lookup");
    println!("listed id: {:?}", listed);
    if first.status == RowStatus::Created {
        assert!(!first.message.contains("no ID"), "{}", first.message);
        assert!(
            first
                .message
                .contains(&listed.expect("group listed").to_string()),
            "{}",
            first.message
        );
    }
    let repeat =
        import_access_group(3, row(), &mut ctx, &live.client, None, false, app.handle()).await;
    show("group repeat (same session)", &repeat);
    assert_eq!(repeat.status, RowStatus::Skipped);
    assert!(
        repeat.message.contains("already exists"),
        "{}",
        repeat.message
    );
    // The group detail endpoint, which migration now uses for scope, reports the real scope.
    let details = live
        .client
        .get_access_group_details(listed.unwrap())
        .await
        .expect("details");
    println!(
        "details: org units {:?}, auto-include {:?}",
        details.get_org_unit_ids(),
        details.get_auto_include()
    );
    assert_eq!(details.get_org_unit_ids(), vec![customer_id]);
}
