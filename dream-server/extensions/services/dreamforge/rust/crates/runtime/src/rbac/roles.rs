//! Role definitions and permission categories.

use serde::{Deserialize, Serialize};

/// Enterprise role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Developer,
    Admin,
}

/// Permission category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    Write,
    Execute,
    Config,
}

impl Role {
    /// Returns the set of permissions granted to this role.
    #[must_use]
    pub fn permissions(self) -> &'static [Permission] {
        match self {
            Self::Viewer => &[Permission::Read],
            Self::Developer => &[Permission::Read, Permission::Write, Permission::Execute],
            Self::Admin => &[
                Permission::Read,
                Permission::Write,
                Permission::Execute,
                Permission::Config,
            ],
        }
    }

    /// Check if this role has a specific permission.
    #[must_use]
    pub fn has_permission(self, perm: Permission) -> bool {
        self.permissions().contains(&perm)
    }
}

/// Map an action string to a permission category.
#[must_use]
pub fn action_to_permission(action: &str) -> Permission {
    if action.starts_with("config.") {
        Permission::Config
    } else if action.contains("execute") || action.starts_with("tool.") {
        Permission::Execute
    } else if action.contains("write") || action.contains("edit") {
        Permission::Write
    } else {
        Permission::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_has_read_only() {
        assert!(Role::Viewer.has_permission(Permission::Read));
        assert!(!Role::Viewer.has_permission(Permission::Write));
        assert!(!Role::Viewer.has_permission(Permission::Execute));
        assert!(!Role::Viewer.has_permission(Permission::Config));
    }

    #[test]
    fn developer_has_read_write_execute() {
        assert!(Role::Developer.has_permission(Permission::Read));
        assert!(Role::Developer.has_permission(Permission::Write));
        assert!(Role::Developer.has_permission(Permission::Execute));
        assert!(!Role::Developer.has_permission(Permission::Config));
    }

    #[test]
    fn admin_has_all() {
        for perm in &[
            Permission::Read,
            Permission::Write,
            Permission::Execute,
            Permission::Config,
        ] {
            assert!(Role::Admin.has_permission(*perm));
        }
    }

    #[test]
    fn action_mapping() {
        assert_eq!(action_to_permission("config.set_model"), Permission::Config);
        assert_eq!(action_to_permission("tool.bash"), Permission::Execute);
        assert_eq!(action_to_permission("file.write"), Permission::Write);
        assert_eq!(action_to_permission("file.read"), Permission::Read);
    }
}
