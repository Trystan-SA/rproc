use crate::monitor::services::{self, ServiceScope};

/// Cached `systemctl show` result for the open unit. `systemctl show` spawns a
/// process, so the app fetches once when the modal opens and reuses this until
/// the user reloads or opens a different unit.
pub struct ServicePropertiesView {
    pub name: String,
    pub scope: ServiceScope,
    pub data: services::ServiceProperties,
}

impl ServicePropertiesView {
    pub fn fetch(name: String, scope: ServiceScope) -> Self {
        let data = services::show_properties(&name, &scope);
        Self { name, scope, data }
    }
}
