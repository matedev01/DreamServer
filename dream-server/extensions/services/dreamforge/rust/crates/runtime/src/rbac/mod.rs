//! Enterprise RBAC: role-based access control layered on top of
//! DreamForge's existing `PermissionPolicy`.
//!
//! Three roles: Admin, Developer, Viewer.
//! Policy rules loaded from YAML define per-action allow/deny overrides.

mod policy;
mod roles;

pub use policy::{PolicyEngine, PolicyRule, RbacDecision};
pub use roles::{Permission, Role};
