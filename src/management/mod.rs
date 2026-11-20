//! Native management parity with the Field web app.
//! Catalogs declare verified contracts; forms never submit arbitrary record JSON.
mod access;
mod accounting;
mod accounting_operations;
mod administration;
mod analytics;
mod analytics_engine;
mod analytics_screen;
mod daily;
mod decisions;
mod detail;
mod files;
mod finance_views;
mod form;
mod gates;
mod insights;
mod intake;
mod inventory_workflows;
mod invoice_workflows;
mod jobcan;
mod model;
mod operations;
mod receipt_pricing;
mod reservation_handoff;
mod reservation_notifications;
mod reservation_settings;
mod reservation_slots;
mod reservation_waitlist;
mod reservation_workflows;
mod sales;
pub(crate) mod schema;
mod screen;
mod workforce;
mod workplace;

pub(crate) use screen::provide_connection;
pub(crate) use screen::{ManagementScreen, ResourceEntry};

pub fn resources() -> Vec<schema::Resource> {
    sales::RESOURCES
        .iter()
        .chain(operations::RESOURCES)
        .chain(insights::RESOURCES)
        .chain(daily::RESOURCES)
        .chain(workplace::RESOURCES)
        .chain(analytics::RESOURCES)
        .chain(intake::RESOURCES)
        .chain(administration::RESOURCES)
        .chain(workforce::RESOURCES)
        .chain(decisions::RESOURCES)
        .chain(accounting::RESOURCES)
        .chain(accounting_operations::RESOURCES)
        .chain(finance_views::RESOURCES)
        .chain(receipt_pricing::RESOURCES)
        .chain(invoice_workflows::RESOURCES)
        .chain(inventory_workflows::RESOURCES)
        .chain(reservation_notifications::RESOURCES)
        .chain(reservation_slots::RESOURCES)
        .chain(reservation_settings::RESOURCES)
        .copied()
        .map(invoice_workflows::decorate)
        .map(inventory_workflows::decorate)
        .map(reservation_waitlist::decorate)
        .map(reservation_settings::decorate)
        .map(reservation_workflows::decorate)
        .collect()
}

pub(crate) fn for_path(path: &str) -> Option<schema::Resource> {
    let base = path.split('?').next().unwrap_or(path);
    resources()
        .into_iter()
        .find(|resource| resource.path.split('?').next() == Some(base))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_catalog_entry_has_a_unique_key_and_known_menu_gate() {
        let resources = resources();
        let mut keys = std::collections::HashSet::new();
        for resource in resources {
            assert!(
                keys.insert(resource.key),
                "duplicate resource {}",
                resource.key
            );
            assert!(
                gates::for_route(resource.web_route).is_some(),
                "unknown route for {}: {}",
                resource.key,
                resource.web_route
            );
            assert!(resource.path.starts_with('/'), "{}", resource.key);
        }
    }
}
