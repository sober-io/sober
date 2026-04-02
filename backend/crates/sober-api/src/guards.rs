//! Reusable authorization guard functions.
//!
//! Layer 2 authorization: services call these after fetching context
//! (membership, plugin, etc.) from the database. Layer 1 (extractors
//! like `AuthUser` and `RequireAdmin`) handles coarse route-level gating.

use sober_auth::AuthUser;
use sober_core::error::AppError;
use sober_core::types::{
    ConversationUser, ConversationUserRole, Plugin, PluginScope, RoleKind, UserId,
};

/// Requires the user to hold the admin role.
pub fn require_admin(user: &AuthUser) -> Result<(), AppError> {
    if user.has_role(&RoleKind::Admin) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Requires the user to hold at least the given conversation role.
///
/// Role hierarchy: Owner > Admin > Member.
pub fn require_conversation_role(
    membership: &ConversationUser,
    minimum: ConversationUserRole,
) -> Result<(), AppError> {
    if role_rank(membership.role) >= role_rank(minimum) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Requires the user to be the conversation owner.
pub fn require_owner(membership: &ConversationUser) -> Result<(), AppError> {
    require_conversation_role(membership, ConversationUserRole::Owner)
}

/// Requires the user to be the conversation owner or the sender of a message.
pub fn require_owner_or_sender(
    membership: &ConversationUser,
    sender_id: Option<UserId>,
    acting_user_id: UserId,
) -> Result<(), AppError> {
    let is_owner = membership.role == ConversationUserRole::Owner;
    let is_sender = sender_id == Some(acting_user_id);
    if is_owner || is_sender {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Checks whether a caller can remove a user with `target_role`.
///
/// Rules: nobody can remove the owner; owner can remove anyone else;
/// admin can only remove members; members cannot remove anyone.
pub fn check_can_remove(
    caller_role: ConversationUserRole,
    target_role: ConversationUserRole,
) -> Result<(), AppError> {
    if target_role == ConversationUserRole::Owner {
        return Err(AppError::Forbidden);
    }
    match caller_role {
        ConversationUserRole::Owner => Ok(()),
        ConversationUserRole::Admin => {
            if target_role != ConversationUserRole::Member {
                return Err(AppError::Forbidden);
            }
            Ok(())
        }
        ConversationUserRole::Member => Err(AppError::Forbidden),
    }
}

/// Checks whether the user can modify (update/delete) a plugin.
///
/// - System plugins: admin only.
/// - User plugins: owner only.
/// - Workspace plugins: owner or system admin.
pub fn can_modify_plugin(user: &AuthUser, plugin: &Plugin) -> Result<(), AppError> {
    match plugin.scope {
        PluginScope::System => require_admin(user),
        PluginScope::User => {
            if plugin.owner_id == Some(user.user_id) {
                Ok(())
            } else {
                Err(AppError::Forbidden)
            }
        }
        PluginScope::Workspace => {
            if plugin.owner_id == Some(user.user_id) {
                Ok(())
            } else {
                require_admin(user)
            }
        }
    }
}

fn role_rank(role: ConversationUserRole) -> u8 {
    match role {
        ConversationUserRole::Member => 1,
        ConversationUserRole::Admin => 2,
        ConversationUserRole::Owner => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::{ConversationId, PluginId, PluginKind, PluginOrigin, PluginStatus};

    fn auth_user(roles: Vec<RoleKind>) -> AuthUser {
        AuthUser {
            user_id: UserId::new(),
            roles,
        }
    }

    fn admin_user() -> AuthUser {
        auth_user(vec![RoleKind::User, RoleKind::Admin])
    }

    fn regular_user() -> AuthUser {
        auth_user(vec![RoleKind::User])
    }

    fn membership(role: ConversationUserRole) -> ConversationUser {
        ConversationUser {
            conversation_id: ConversationId::new(),
            user_id: UserId::new(),
            role,
            joined_at: chrono::Utc::now(),
            unread_count: 0,
            last_read_at: None,
            last_read_message_id: None,
        }
    }

    fn test_plugin(scope: PluginScope, owner_id: Option<UserId>) -> Plugin {
        Plugin {
            id: PluginId::new(),
            name: "test".into(),
            kind: PluginKind::Mcp,
            version: None,
            description: None,
            origin: PluginOrigin::User,
            scope,
            owner_id,
            workspace_id: None,
            status: PluginStatus::Enabled,
            config: serde_json::json!({}),
            installed_by: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn require_admin_passes_for_admin() {
        assert!(require_admin(&admin_user()).is_ok());
    }

    #[test]
    fn require_admin_fails_for_regular_user() {
        assert!(require_admin(&regular_user()).is_err());
    }

    #[test]
    fn owner_satisfies_any_minimum() {
        let m = membership(ConversationUserRole::Owner);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Owner).is_ok());
    }

    #[test]
    fn admin_satisfies_admin_and_member() {
        let m = membership(ConversationUserRole::Admin);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Owner).is_err());
    }

    #[test]
    fn member_only_satisfies_member() {
        let m = membership(ConversationUserRole::Member);
        assert!(require_conversation_role(&m, ConversationUserRole::Member).is_ok());
        assert!(require_conversation_role(&m, ConversationUserRole::Admin).is_err());
    }

    #[test]
    fn require_owner_passes_for_owner() {
        assert!(require_owner(&membership(ConversationUserRole::Owner)).is_ok());
    }

    #[test]
    fn require_owner_fails_for_non_owner() {
        assert!(require_owner(&membership(ConversationUserRole::Admin)).is_err());
        assert!(require_owner(&membership(ConversationUserRole::Member)).is_err());
    }

    #[test]
    fn owner_can_always_act() {
        let m = membership(ConversationUserRole::Owner);
        let other = UserId::new();
        assert!(require_owner_or_sender(&m, Some(other), m.user_id).is_ok());
    }

    #[test]
    fn sender_can_act_on_own_message() {
        let user_id = UserId::new();
        let mut m = membership(ConversationUserRole::Member);
        m.user_id = user_id;
        assert!(require_owner_or_sender(&m, Some(user_id), user_id).is_ok());
    }

    #[test]
    fn non_owner_non_sender_is_rejected() {
        let m = membership(ConversationUserRole::Member);
        let other = UserId::new();
        assert!(require_owner_or_sender(&m, Some(other), m.user_id).is_err());
    }

    #[test]
    fn owner_can_remove_admin_and_member() {
        assert!(check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Admin).is_ok());
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Member).is_ok()
        );
    }

    #[test]
    fn nobody_can_remove_owner() {
        assert!(
            check_can_remove(ConversationUserRole::Owner, ConversationUserRole::Owner).is_err()
        );
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Owner).is_err()
        );
    }

    #[test]
    fn admin_can_remove_member_only() {
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Member).is_ok()
        );
        assert!(
            check_can_remove(ConversationUserRole::Admin, ConversationUserRole::Admin).is_err()
        );
    }

    #[test]
    fn member_cannot_remove_anyone() {
        assert!(
            check_can_remove(ConversationUserRole::Member, ConversationUserRole::Member).is_err()
        );
    }

    #[test]
    fn admin_can_modify_system_plugin() {
        let user = admin_user();
        let plugin = test_plugin(PluginScope::System, None);
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn regular_user_cannot_modify_system_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::System, None);
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }

    #[test]
    fn owner_can_modify_own_user_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::User, Some(user.user_id));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn non_owner_cannot_modify_user_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::User, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }

    #[test]
    fn owner_can_modify_workspace_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(user.user_id));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn admin_can_modify_any_workspace_plugin() {
        let user = admin_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_ok());
    }

    #[test]
    fn non_owner_non_admin_cannot_modify_workspace_plugin() {
        let user = regular_user();
        let plugin = test_plugin(PluginScope::Workspace, Some(UserId::new()));
        assert!(can_modify_plugin(&user, &plugin).is_err());
    }
}
