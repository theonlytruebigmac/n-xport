//! The name lookups every import handler resolves rows against.

use crate::api::NcClient;
use crate::models::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

/// Pre-fetched lookup data needed across most handlers.
pub struct ImportContext {
    pub source_so_id: i64,
    /// Connected SO's name, lowercased. Used so users can put the SO name in
    /// `customerName` to place a user at SO level.
    pub source_so_name: Option<String>,
    /// customer name (lowercase) -> customer id
    pub customers_by_name: HashMap<String, i64>,
    /// (customer name lowercase, site name lowercase) -> site id
    pub sites_by_name: HashMap<(String, String), i64>,
    /// site name only (lowercase) -> site id. First-seen wins on collisions.
    pub sites_by_name_flat: HashMap<String, i64>,
    /// role name (lowercase) -> role id (any OU; first-seen wins)
    pub roles_by_name: HashMap<String, i64>,
    /// group name (lowercase) -> group id (any OU; first-seen wins)
    pub groups_by_name: HashMap<String, i64>,
    /// org unit id -> role name (lowercase) -> role id, as visible at that org
    /// unit (its own roles plus ones inherited from the SO). Same-named roles at
    /// sibling org units are distinct, so existence checks use this, not
    /// `roles_by_name`. Only SO and customer OUs are loaded.
    pub roles_by_ou: HashMap<i64, HashMap<String, i64>>,
    /// Same as `roles_by_ou`, for access groups.
    pub groups_by_ou: HashMap<i64, HashMap<String, i64>>,
    /// permission name -> permission id (the list bundled in `data/rolePermissionIds.csv`)
    pub permission_lookup: PermissionLookup,
    pub org_properties_by_ou: HashMap<i64, Vec<OrgProperty>>,
    pub devices_by_ou: HashMap<i64, Vec<Device>>,
    pub device_properties_by_id: HashMap<i64, Vec<DeviceProperty>>,
    /// Resources created this session whose response carried no ID, keyed
    /// `kind:name`. They cannot live in the ID lookup maps, so this set stops
    /// repeated rows (or a re-run against the cached context) from creating
    /// the same resource twice.
    pub unconfirmed_creates: HashSet<String>,
    /// `login@org-unit` keys of users created this session. Re-running the
    /// same CSV must not depend on the server rejecting the duplicate.
    pub created_users: HashSet<String>,
}

impl ImportContext {
    /// Build the lookup tables for the target service org.
    pub async fn load(
        client: &NcClient,
        source_so_id: i64,
        cancel_token: &AtomicBool,
    ) -> Result<Self, String> {
        if cancel_token.load(Ordering::Relaxed) {
            return Err("Import cancelled".into());
        }
        let mut customers_by_name = HashMap::new();
        let customers = client
            .get_customers_by_so(source_so_id)
            .await
            .map_err(|e| format!("Failed to fetch customers: {}", e))?;
        if cancel_token.load(Ordering::Relaxed) {
            return Err("Import cancelled".into());
        }
        for c in &customers {
            customers_by_name.insert(c.customer_name.to_lowercase(), c.customer_id);
        }

        // Build (customer_name, site_name) -> site_id map. Sites only carry parent IDs,
        // so we resolve the parent customer's name from the just-fetched customer list.
        // Also build a flat site-name -> id map for the user importer's relaxed lookup
        // (first-seen wins when site names collide across customers).
        let mut sites_by_name = HashMap::new();
        let mut sites_by_name_flat: HashMap<String, i64> = HashMap::new();
        let cust_id_to_name: HashMap<i64, String> = customers
            .iter()
            .map(|c| (c.customer_id, c.customer_name.to_lowercase()))
            .collect();
        if let Ok(sites) = client.get_sites_by_so(source_so_id).await {
            // `/api/sites` lists every site on the server, not just this service org's,
            // so only sites under this service org's customers may be looked up. Others
            // belong to different service orgs and must never be a placement target.
            for s in sites {
                let parent_id = s.parent_id.or(s.customer_id).or(s.customerid);
                let Some(parent_name) = parent_id.and_then(|pid| cust_id_to_name.get(&pid)) else {
                    continue;
                };
                let site_key = s.site_name.to_lowercase();
                sites_by_name_flat
                    .entry(site_key.clone())
                    .or_insert(s.site_id);
                sites_by_name.insert((parent_name.clone(), site_key), s.site_id);
            }
        }
        if cancel_token.load(Ordering::Relaxed) {
            return Err("Import cancelled".into());
        }

        // Roles & groups: walk SO + every customer to populate name -> id maps.
        // First-seen wins because inherited roles appear at every child OU.
        let mut roles_by_name = HashMap::new();
        let mut groups_by_name = HashMap::new();
        let mut roles_by_ou: HashMap<i64, HashMap<String, i64>> = HashMap::new();
        let mut groups_by_ou: HashMap<i64, HashMap<String, i64>> = HashMap::new();
        let mut all_ous: Vec<i64> = vec![source_so_id];
        all_ous.extend(customers.iter().map(|c| c.customer_id));
        for ou_id in &all_ous {
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Import cancelled".into());
            }
            if let Ok(roles) = client.get_user_roles(*ou_id).await {
                let visible = roles_by_ou.entry(*ou_id).or_default();
                for r in roles {
                    if let Some(name) = r.role_name {
                        visible.insert(name.to_lowercase(), r.role_id);
                        roles_by_name
                            .entry(name.to_lowercase())
                            .or_insert(r.role_id);
                    }
                }
            }
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Import cancelled".into());
            }
            if let Ok(groups) = client.get_access_groups(*ou_id).await {
                let visible = groups_by_ou.entry(*ou_id).or_default();
                for g in groups {
                    if let Some(name) = g.group_name {
                        visible.insert(name.to_lowercase(), g.group_id);
                        groups_by_name
                            .entry(name.to_lowercase())
                            .or_insert(g.group_id);
                    }
                }
            }
        }

        // Load permission name->id lookup from the bundled CSV (same one migration uses)
        let permission_lookup = PermissionLookup::bundled();

        // Best-effort fetch of the connected SO's name so users can put the SO
        // name in the user-import `customerName` column.
        let source_so_name = if cancel_token.load(Ordering::Relaxed) {
            None
        } else {
            client
                .get_service_org_by_id(source_so_id)
                .await
                .ok()
                .map(|so| so.so_name.to_lowercase())
        };

        Ok(Self {
            source_so_id,
            source_so_name,
            customers_by_name,
            sites_by_name,
            sites_by_name_flat,
            roles_by_name,
            groups_by_name,
            roles_by_ou,
            groups_by_ou,
            permission_lookup,
            org_properties_by_ou: HashMap::new(),
            devices_by_ou: HashMap::new(),
            device_properties_by_id: HashMap::new(),
            unconfirmed_creates: HashSet::new(),
            created_users: HashSet::new(),
        })
    }

    /// An empty context for tests that only need one to exist.
    #[cfg(test)]
    pub(crate) fn for_tests(source_so_id: i64) -> Self {
        Self {
            source_so_id,
            source_so_name: None,
            customers_by_name: HashMap::new(),
            sites_by_name: HashMap::new(),
            sites_by_name_flat: HashMap::new(),
            roles_by_name: HashMap::new(),
            groups_by_name: HashMap::new(),
            roles_by_ou: HashMap::new(),
            groups_by_ou: HashMap::new(),
            permission_lookup: PermissionLookup::from_csv(""),
            org_properties_by_ou: HashMap::new(),
            devices_by_ou: HashMap::new(),
            device_properties_by_id: HashMap::new(),
            unconfirmed_creates: HashSet::new(),
            created_users: HashSet::new(),
        }
    }

    /// ID of the role named `key` (lowercase) visible at `ou`, if known.
    pub(super) fn role_at(&self, ou: i64, key: &str) -> Option<i64> {
        self.roles_by_ou.get(&ou)?.get(key).copied()
    }

    /// ID of the access group named `key` (lowercase) visible at `ou`, if known.
    pub(super) fn group_at(&self, ou: i64, key: &str) -> Option<i64> {
        self.groups_by_ou.get(&ou)?.get(key).copied()
    }

    /// Record a role created at `ou`. Roles created at the SO are inherited by
    /// every customer; roles created at a customer stay at that customer.
    pub(super) fn record_role(&mut self, ou: i64, key: String, id: i64) {
        Self::record_scoped(&mut self.roles_by_ou, self.source_so_id, ou, &key, id);
        self.roles_by_name.entry(key).or_insert(id);
    }

    /// Record an access group created at `ou` (same visibility rules as roles).
    pub(super) fn record_group(&mut self, ou: i64, key: String, id: i64) {
        Self::record_scoped(&mut self.groups_by_ou, self.source_so_id, ou, &key, id);
        self.groups_by_name.entry(key).or_insert(id);
    }

    pub(super) fn record_scoped(
        by_ou: &mut HashMap<i64, HashMap<String, i64>>,
        so_id: i64,
        ou: i64,
        key: &str,
        id: i64,
    ) {
        if ou == so_id {
            for visible in by_ou.values_mut() {
                visible.entry(key.to_string()).or_insert(id);
            }
        }
        by_ou.entry(ou).or_default().insert(key.to_string(), id);
    }

    /// Whether a create with no returned ID already happened at `ou` or, for
    /// inherited resources, at the SO.
    pub(super) fn unconfirmed_at(&self, kind: &str, ou: i64, key: &str) -> bool {
        self.unconfirmed_creates
            .contains(&format!("{}:{}:{}", kind, ou, key))
            || self
                .unconfirmed_creates
                .contains(&format!("{}:{}:{}", kind, self.source_so_id, key))
    }
}
