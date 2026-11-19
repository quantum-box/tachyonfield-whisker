use super::{gates, model, schema::Resource};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Access {
    pub capabilities: Value,
    pub flags: HashSet<String>,
    pub deliveries: HashSet<String>,
    pub saas: bool,
    pub actor_id: Option<String>,
    pub notice: String,
}

impl Access {
    pub fn load(token: &str, tenant: &Tenant) -> Result<Self, String> {
        let (capabilities, flags, deliveries, saas, actor) = std::thread::scope(|scope| {
            let capabilities =
                scope.spawn(|| api::fetch_record(token, tenant, "/v1/field/client-capabilities"));
            let flags = scope.spawn(|| {
                api::send_json(
                    token,
                    tenant,
                    "POST",
                    "/v1/feature-flags/evaluate",
                    &json!({"keys": gates::feature_flags()}),
                    Duration::from_secs(15),
                )
            });
            let deliveries = scope.spawn(|| {
                api::send_json(
                    token,
                    tenant,
                    "POST",
                    "/v1/feature-deliveries/evaluate",
                    &json!({"keys": gates::delivery_keys()}),
                    Duration::from_secs(15),
                )
            });
            let saas = scope
                .spawn(|| api::fetch_record(token, tenant, "/v1/erp/it-governance/availability"));
            let actor = scope
                .spawn(|| api::fetch_record(token, tenant, "/v1/erp/me?extensionKey=golf_course"));
            let failed = || Err("利用権限を確認できませんでした".to_owned());
            (
                capabilities.join().unwrap_or_else(|_| failed()),
                flags.join().unwrap_or_else(|_| failed()),
                deliveries.join().unwrap_or_else(|_| failed()),
                saas.join().unwrap_or_else(|_| failed()),
                actor.join().unwrap_or_else(|_| failed()),
            )
        });
        let capabilities = capabilities?;
        let notice = if flags.is_err() || deliveries.is_err() {
            "一部の機能の利用可否を確認できません。時間をおいて再確認してください".into()
        } else {
            String::new()
        };
        let saas = saas.unwrap_or(Value::Null);
        let actor = actor.unwrap_or(Value::Null);
        Ok(Self {
            capabilities,
            flags: enabled(&flags.unwrap_or(Value::Null), false),
            deliveries: enabled(&deliveries.unwrap_or(Value::Null), true),
            saas: saas["enabled"].as_bool() == Some(true)
                && saas["canRead"].as_bool() == Some(true),
            actor_id: actor["user"]["id"]
                .as_str()
                .filter(|id| id.starts_with("us_") && id.len() > 3)
                .map(str::to_owned),
            notice,
        })
    }

    pub fn allows_route(&self, route: &str, tenant_id: &str) -> bool {
        let Some(gate) = gates::for_route(route) else {
            return false;
        };
        if gate.only_tenant_id.is_some_and(|id| id != tenant_id) {
            return false;
        }
        if gate
            .feature_flag
            .is_some_and(|key| !self.flags.contains(key))
        {
            return false;
        }
        if !gate.delivery_keys.is_empty()
            && !gate
                .delivery_keys
                .iter()
                .any(|key| self.deliveries.contains(*key))
        {
            return false;
        }
        if gate.saas_availability && !self.saas {
            return false;
        }
        // Match the Web policy for older API builds: explicit denials hide menus;
        // individual reads and writes are always authorized by the server.
        let denied = self.capabilities["deniedMenuActions"].as_array();
        gate.actions.is_empty()
            || gate.actions.iter().any(|action| {
                !denied.is_some_and(|items| items.iter().any(|item| item.as_str() == Some(action)))
            })
    }

    pub fn allows(&self, resource: Resource, tenant_id: &str) -> bool {
        self.allows_route(resource.web_route, tenant_id)
            && resource
                .read_capability
                .is_none_or(|key| model::value_at(&self.capabilities, key).as_bool() == Some(true))
    }

    pub fn can_mutate(
        &self,
        operation: super::schema::Mutation,
        collection: &Value,
        original: &Value,
    ) -> bool {
        model::capability_allowed(operation, &self.capabilities, collection)
            && operation.is_available(original)
            && (operation.actor_body_field.is_none() || self.actor_id.is_some())
            && operation.actor_field.is_none_or(|field| {
                self.actor_id
                    .as_deref()
                    .is_some_and(|actor| model::value_at(original, field).as_str() == Some(actor))
            })
            && operation.actor_excluded_field.is_none_or(|field| {
                self.actor_id.as_deref().is_some_and(|actor| {
                    model::value_at(original, field)
                        .as_str()
                        .is_some_and(|owner| !owner.is_empty() && owner != actor)
                })
            })
    }
}

fn enabled(response: &Value, require_resolved: bool) -> HashSet<String> {
    response["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| {
            value["enabled"].as_bool() == Some(true)
                && (!require_resolved || value["resolved"].as_bool() == Some(true))
        })
        .filter_map(|value| value["key"].as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unresolved_contracts_and_unknown_routes_never_grant_access() {
        assert!(
            enabled(
                &json!({"values":[{"key":"a","enabled":true,"resolved":false}]}),
                true
            )
            .is_empty()
        );
        assert!(!Access::default().allows_route("unknown-management-route", "tenant"));
        assert!(!Access::default().allows_route("strategy", "tenant"));
    }
    #[test]
    fn explicit_denials_hide_existing_business_menus() {
        let access = Access {
            capabilities: json!({"deniedMenuActions":["deal:List"]}),
            ..Access::default()
        };
        assert!(!access.allows_route("deals", "tenant"));
        assert!(access.allows_route("library/clients", "tenant"));
    }
}
