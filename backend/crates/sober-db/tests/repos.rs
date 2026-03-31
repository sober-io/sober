//! Integration tests for all Pg*Repo implementations.
//!
//! These tests run against a real PostgreSQL instance using `#[sqlx::test]`,
//! which creates a temporary database per test and runs migrations automatically.
//!
//! Requires: `DATABASE_URL` env var pointing to a PostgreSQL instance.
//! Run: `DATABASE_URL=postgres://sober:sober@localhost:5432/sober cargo test -p sober-db -q`

use chrono::Utc;
use sober_core::types::*;
use sober_db::*;
use sqlx::PgPool;

// ── Users ────────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn user_create_and_get_by_id(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "test@example.com".into(),
        username: "testuser".into(),
        password_hash: "argon2id$hash".into(),
    };

    let user = repo.create(input).await.unwrap();
    assert_eq!(user.email, "test@example.com");
    assert_eq!(user.username, "testuser");
    assert_eq!(user.status, UserStatus::Pending);

    let fetched = repo.get_by_id(user.id).await.unwrap();
    assert_eq!(fetched.id, user.id);
    assert_eq!(fetched.email, "test@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_get_by_email_and_username(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "find@example.com".into(),
        username: "findme".into(),
        password_hash: "hash".into(),
    };
    repo.create(input).await.unwrap();

    let by_email = repo.get_by_email("find@example.com").await.unwrap();
    assert_eq!(by_email.username, "findme");

    let by_username = repo.get_by_username("findme").await.unwrap();
    assert_eq!(by_username.email, "find@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_create_duplicate_email_returns_conflict(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "dup@example.com".into(),
        username: "user1".into(),
        password_hash: "hash".into(),
    };
    repo.create(input).await.unwrap();

    let input2 = CreateUser {
        email: "dup@example.com".into(),
        username: "user2".into(),
        password_hash: "hash".into(),
    };
    let err = repo.create(input2).await.unwrap_err();
    assert!(matches!(err, sober_core::error::AppError::Conflict(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_create_with_roles_assigns_single_role(pool: PgPool) {
    let repo = PgUserRepo::new(pool.clone());
    let input = CreateUser {
        email: "roleuser@example.com".into(),
        username: "roleuser".into(),
        password_hash: "hash".into(),
    };

    let user = repo
        .create_with_roles(input, &[RoleKind::User])
        .await
        .unwrap();
    assert_eq!(user.status, UserStatus::Pending);

    // Verify role was assigned by checking the user_roles table directly
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user_roles WHERE user_id = $1")
        .bind(user.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_create_with_roles_assigns_multiple_roles(pool: PgPool) {
    let repo = PgUserRepo::new(pool.clone());
    let input = CreateUser {
        email: "multirole@example.com".into(),
        username: "multirole".into(),
        password_hash: "hash".into(),
    };

    let user = repo
        .create_with_roles(input, &[RoleKind::User, RoleKind::Admin])
        .await
        .unwrap();
    assert_eq!(user.status, UserStatus::Pending);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user_roles WHERE user_id = $1")
        .bind(user.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_create_with_nonexistent_role_returns_not_found(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "norole@example.com".into(),
        username: "norole".into(),
        password_hash: "hash".into(),
    };

    let err = repo
        .create_with_roles(input, &[RoleKind::Custom("nonexistent".into())])
        .await
        .unwrap_err();
    assert!(matches!(err, sober_core::error::AppError::NotFound(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_update_status(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "status@example.com".into(),
        username: "statususer".into(),
        password_hash: "hash".into(),
    };
    let user = repo.create(input).await.unwrap();

    repo.update_status(user.id, UserStatus::Active)
        .await
        .unwrap();
    let updated = repo.get_by_id(user.id).await.unwrap();
    assert_eq!(updated.status, UserStatus::Active);
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_get_password_hash(pool: PgPool) {
    let repo = PgUserRepo::new(pool);
    let input = CreateUser {
        email: "pw@example.com".into(),
        username: "pwuser".into(),
        password_hash: "argon2id$thehash".into(),
    };
    let user = repo.create(input).await.unwrap();

    let hash = repo.get_password_hash(user.id).await.unwrap();
    assert_eq!(hash, "argon2id$thehash");
}

// ── Sessions ─────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn session_create_and_lookup(pool: PgPool) {
    let repo = PgSessionRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "sess@example.com".into(),
            username: "sessuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let session = repo
        .create(CreateSession {
            user_id: user.id,
            token_hash: "abc123hash".into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        })
        .await
        .unwrap();

    let found = repo.get_by_token_hash("abc123hash").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, session.id);

    let not_found = repo.get_by_token_hash("nonexistent").await.unwrap();
    assert!(not_found.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn session_delete_and_cleanup(pool: PgPool) {
    let repo = PgSessionRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "cleanup@example.com".into(),
            username: "cleanupuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    // Create an already-expired session
    repo.create(CreateSession {
        user_id: user.id,
        token_hash: "expired_hash".into(),
        expires_at: Utc::now() - chrono::Duration::hours(1),
    })
    .await
    .unwrap();

    // Expired session should not be found
    let found = repo.get_by_token_hash("expired_hash").await.unwrap();
    assert!(found.is_none());

    // Cleanup should remove it
    let removed = repo.cleanup_expired().await.unwrap();
    assert_eq!(removed, 1);

    // Delete by token hash
    repo.create(CreateSession {
        user_id: user.id,
        token_hash: "to_delete".into(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
    })
    .await
    .unwrap();

    repo.delete_by_token_hash("to_delete").await.unwrap();
    let gone = repo.get_by_token_hash("to_delete").await.unwrap();
    assert!(gone.is_none());
}

// ── Conversations & Messages ─────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn conversation_crud(pool: PgPool) {
    let conv_repo = PgConversationRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "conv@example.com".into(),
            username: "convuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let conv = conv_repo
        .create(user.id, Some("Test Chat"), None)
        .await
        .unwrap();
    assert_eq!(conv.title.as_deref(), Some("Test Chat"));

    let fetched = conv_repo.get_by_id(conv.id).await.unwrap();
    assert_eq!(fetched.title, conv.title);

    let list = conv_repo.list_by_user(user.id).await.unwrap();
    assert_eq!(list.len(), 1);

    conv_repo
        .update_title(conv.id, "Updated Title")
        .await
        .unwrap();
    let updated = conv_repo.get_by_id(conv.id).await.unwrap();
    assert_eq!(updated.title.as_deref(), Some("Updated Title"));

    conv_repo.delete(conv.id).await.unwrap();
    let err = conv_repo.get_by_id(conv.id).await.unwrap_err();
    assert!(matches!(err, sober_core::error::AppError::NotFound(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn message_create_and_list(pool: PgPool) {
    let msg_repo = PgMessageRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "msg@example.com".into(),
            username: "msguser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let conv = conv_repo.create(user.id, None, None).await.unwrap();

    let msg = msg_repo
        .create(CreateMessage {
            conversation_id: conv.id,
            role: MessageRole::User,
            content: vec![sober_core::types::ContentBlock::text("Hello")],
            reasoning: None,
            token_count: Some(5),
            metadata: None,
            user_id: None,
        })
        .await
        .unwrap();
    assert_eq!(msg.text_content(), "Hello");
    assert_eq!(msg.role, MessageRole::User);

    msg_repo
        .create(CreateMessage {
            conversation_id: conv.id,
            role: MessageRole::Assistant,
            content: vec![sober_core::types::ContentBlock::text("Hi there")],
            reasoning: None,
            token_count: Some(8),
            metadata: None,
            user_id: None,
        })
        .await
        .unwrap();

    let messages = msg_repo.list_by_conversation(conv.id, 10).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text_content(), "Hello"); // oldest first
    assert_eq!(messages[1].text_content(), "Hi there");
}

// ── Jobs ─────────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn job_lifecycle(pool: PgPool) {
    let repo = PgJobRepo::new(pool);

    let job = repo
        .create(CreateJob {
            name: "test_job".into(),
            schedule: "0 * * * *".into(),
            payload: serde_json::json!({"task": "prune"}),
            owner_type: "system".into(),
            owner_id: None,
            workspace_id: None,
            created_by: None,
            conversation_id: None,
            next_run_at: Utc::now() + chrono::Duration::hours(1),
        })
        .await
        .unwrap();
    assert_eq!(job.name, "test_job");
    assert_eq!(job.status, JobStatus::Active);

    let fetched = repo.get_by_id(job.id).await.unwrap();
    assert_eq!(fetched.id, job.id);

    let active = repo.list_active().await.unwrap();
    assert_eq!(active.len(), 1);

    let now = Utc::now();
    repo.mark_last_run(job.id, now).await.unwrap();
    let updated = repo.get_by_id(job.id).await.unwrap();
    assert!(updated.last_run_at.is_some());

    repo.cancel(job.id).await.unwrap();
    let cancelled = repo.get_by_id(job.id).await.unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);

    let active_after = repo.list_active().await.unwrap();
    assert!(active_after.is_empty());
}

// ── Workspaces ───────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn workspace_lifecycle(pool: PgPool) {
    let repo = PgWorkspaceRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "ws@example.com".into(),
            username: "wsuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let ws = repo
        .create(user.id, "My Project", None, "/home/user/project")
        .await
        .unwrap();
    assert_eq!(ws.name, "My Project");
    assert_eq!(ws.state, WorkspaceState::Active);

    let list = repo.list_by_user(user.id).await.unwrap();
    assert_eq!(list.len(), 1);

    repo.archive(ws.id).await.unwrap();
    let list_after_archive = repo.list_by_user(user.id).await.unwrap();
    assert_eq!(list_after_archive.len(), 1); // archived workspaces still included, only deleted filtered out

    repo.restore(ws.id).await.unwrap();
    let list_after_restore = repo.list_by_user(user.id).await.unwrap();
    assert_eq!(list_after_restore.len(), 1);

    repo.delete(ws.id).await.unwrap();
    let deleted = repo.get_by_id(ws.id).await.unwrap();
    assert_eq!(deleted.state, WorkspaceState::Deleted);
}

// ── Workspace Repos & Worktrees ──────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn workspace_repo_and_worktree(pool: PgPool) {
    let ws_repo = PgWorkspaceRepo::new(pool.clone());
    let repo_repo = PgWorkspaceRepoRepo::new(pool.clone());
    let wt_repo = PgWorktreeRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "wt@example.com".into(),
            username: "wtuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let ws = ws_repo
        .create(user.id, "WS", None, "/tmp/ws")
        .await
        .unwrap();

    // Register a linked repo so find_by_linked_path can find it
    let repo = repo_repo
        .register(
            ws.id,
            RegisterRepo {
                name: "my-repo".into(),
                path: "/tmp/ws/my-repo".into(),
                is_linked: true,
                remote_url: Some("https://github.com/user/my-repo".into()),
                default_branch: "main".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(repo.name, "my-repo");

    let found = repo_repo
        .find_by_linked_path("/tmp/ws/my-repo", user.id)
        .await
        .unwrap();
    assert!(found.is_some());

    let not_found = repo_repo
        .find_by_linked_path("/nonexistent", user.id)
        .await
        .unwrap();
    assert!(not_found.is_none());

    let repos = repo_repo.list_by_workspace(ws.id).await.unwrap();
    assert_eq!(repos.len(), 1);

    // Worktrees
    let wt = wt_repo
        .create(
            repo.id,
            "feat/test",
            "/tmp/ws/.worktrees/test",
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(wt.state, WorktreeState::Active);

    let wts = wt_repo.list_by_repo(repo.id).await.unwrap();
    assert_eq!(wts.len(), 1);

    wt_repo.mark_stale(wt.id).await.unwrap();

    // list_stale_candidates finds worktrees with state=active and last_active_at older than threshold
    // Our worktree is now state=stale so it shouldn't appear
    let stale = wt_repo
        .list_stale_candidates(Utc::now() + chrono::Duration::hours(1))
        .await
        .unwrap();
    assert!(stale.is_empty());

    wt_repo.delete(wt.id).await.unwrap();
    let wts_after = wt_repo.list_by_repo(repo.id).await.unwrap();
    assert!(wts_after.is_empty());
}

// ── Artifacts ────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn artifact_crud_and_relations(pool: PgPool) {
    let art_repo = PgArtifactRepo::new(pool.clone());
    let ws_repo = PgWorkspaceRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "art@example.com".into(),
            username: "artuser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let ws = ws_repo
        .create(user.id, "ArtWS", None, "/tmp/art")
        .await
        .unwrap();

    let art1 = art_repo
        .create(CreateArtifact {
            workspace_id: ws.id,
            user_id: user.id,
            kind: ArtifactKind::CodeChange,
            title: "main.rs".into(),
            storage_type: "inline".into(),
            inline_content: Some("fn main() {}".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(art1.state, ArtifactState::Draft);

    let art2 = art_repo
        .create(CreateArtifact {
            workspace_id: ws.id,
            user_id: user.id,
            kind: ArtifactKind::Document,
            title: "README.md".into(),
            storage_type: "inline".into(),
            inline_content: Some("# README".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    // Filter by kind
    let code_only = art_repo
        .list_by_workspace(
            ws.id,
            ArtifactFilter {
                kind: Some(ArtifactKind::CodeChange),
                state: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(code_only.len(), 1);
    assert_eq!(code_only[0].title, "main.rs");

    // No filter
    let all = art_repo
        .list_by_workspace(ws.id, ArtifactFilter::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 2);

    // Update state
    art_repo
        .update_state(art1.id, ArtifactState::Proposed)
        .await
        .unwrap();
    let updated = art_repo.get_by_id(art1.id).await.unwrap();
    assert_eq!(updated.state, ArtifactState::Proposed);

    // Add relation (idempotent)
    art_repo
        .add_relation(art2.id, art1.id, ArtifactRelation::SpawnedBy)
        .await
        .unwrap();
    art_repo
        .add_relation(art2.id, art1.id, ArtifactRelation::SpawnedBy)
        .await
        .unwrap(); // no error on duplicate
}

// ── Audit Log ────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../migrations")]
async fn audit_log_create_and_list(pool: PgPool) {
    let repo = PgAuditLogRepo::new(pool.clone());
    let user_repo = PgUserRepo::new(pool);

    let user = user_repo
        .create(CreateUser {
            email: "audit@example.com".into(),
            username: "audituser".into(),
            password_hash: "hash".into(),
        })
        .await
        .unwrap();

    let entry = repo
        .create(CreateAuditLog {
            actor_id: Some(user.id),
            action: "user.login".into(),
            target_type: Some("session".into()),
            target_id: None,
            details: Some(serde_json::json!({"method": "password"})),
            ip_address: Some("192.168.1.1".into()),
        })
        .await
        .unwrap();
    assert_eq!(entry.action, "user.login");
    assert_eq!(entry.ip_address.as_deref(), Some("192.168.1.1"));

    repo.create(CreateAuditLog {
        actor_id: None,
        action: "system.startup".into(),
        target_type: None,
        target_id: None,
        details: None,
        ip_address: None,
    })
    .await
    .unwrap();

    let recent = repo.list_recent(10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].action, "system.startup"); // newest first

    let by_actor = repo.list_by_actor(user.id, 10).await.unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(by_actor[0].action, "user.login");
}

// ── Message Search ──────────────────────────────────────────────────────────

/// Helper: create a user, returning the `User`.
async fn create_test_user(
    user_repo: &PgUserRepo,
    email: &str,
    username: &str,
) -> Result<sober_core::types::User, sober_core::error::AppError> {
    user_repo
        .create(CreateUser {
            email: email.into(),
            username: username.into(),
            password_hash: "argon2id$testhash".into(),
        })
        .await
}

/// Helper: insert a message into a conversation.
async fn insert_message(
    msg_repo: &PgMessageRepo,
    conversation_id: ConversationId,
    content: &str,
) -> Result<sober_core::types::Message, sober_core::error::AppError> {
    msg_repo
        .create(CreateMessage {
            conversation_id,
            role: MessageRole::User,
            content: vec![sober_core::types::ContentBlock::text(content)],
            reasoning: None,
            token_count: None,
            metadata: None,
            user_id: None,
        })
        .await
}

#[sqlx::test(migrations = "../../migrations")]
async fn search_by_user_returns_matching_messages(
    pool: PgPool,
) -> Result<(), sober_core::error::AppError> {
    let user_repo = PgUserRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let msg_repo = PgMessageRepo::new(pool);

    let user = create_test_user(&user_repo, "search@example.com", "searchuser").await?;
    let conv = conv_repo.create(user.id, Some("Dev Chat"), None).await?;

    insert_message(&msg_repo, conv.id, "Rust borrow checker discussion").await?;
    insert_message(&msg_repo, conv.id, "Python type hints").await?;
    insert_message(&msg_repo, conv.id, "Kubernetes deployment strategy").await?;

    let results = msg_repo
        .search_by_user(user.id, "Rust borrow", None, 10)
        .await?;

    assert!(!results.is_empty(), "expected at least one search hit");
    assert!(
        results.iter().any(|h| h.content.iter().any(|b| matches!(b, sober_core::types::ContentBlock::Text { text } if text.contains("Rust borrow")))),
        "expected a hit containing 'Rust borrow', got: {results:?}"
    );
    // Verify the hit carries correct conversation metadata.
    assert_eq!(results[0].conversation_id, conv.id);
    assert_eq!(results[0].conversation_title.as_deref(), Some("Dev Chat"));

    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn search_by_user_scoped_to_conversation(
    pool: PgPool,
) -> Result<(), sober_core::error::AppError> {
    let user_repo = PgUserRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let msg_repo = PgMessageRepo::new(pool);

    let user = create_test_user(&user_repo, "scope@example.com", "scopeuser").await?;
    let conv_a = conv_repo.create(user.id, Some("Chat A"), None).await?;
    let conv_b = conv_repo.create(user.id, Some("Chat B"), None).await?;

    insert_message(&msg_repo, conv_a.id, "database indexing strategies").await?;
    insert_message(&msg_repo, conv_b.id, "database migration patterns").await?;

    // Search scoped to conv_a — should only return the indexing message.
    let results = msg_repo
        .search_by_user(user.id, "database", Some(conv_a.id), 10)
        .await?;

    assert!(!results.is_empty(), "expected at least one hit in conv_a");
    for hit in &results {
        assert_eq!(
            hit.conversation_id, conv_a.id,
            "all hits must belong to conv_a"
        );
    }

    // Search scoped to conv_b — should only return the migration message.
    let results_b = msg_repo
        .search_by_user(user.id, "database", Some(conv_b.id), 10)
        .await?;

    assert!(!results_b.is_empty(), "expected at least one hit in conv_b");
    for hit in &results_b {
        assert_eq!(
            hit.conversation_id, conv_b.id,
            "all hits must belong to conv_b"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn search_by_user_no_cross_user_leakage(
    pool: PgPool,
) -> Result<(), sober_core::error::AppError> {
    let user_repo = PgUserRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let msg_repo = PgMessageRepo::new(pool);

    let alice = create_test_user(&user_repo, "alice@example.com", "alice").await?;
    let bob = create_test_user(&user_repo, "bob@example.com", "bob").await?;

    let alice_conv = conv_repo.create(alice.id, Some("Alice Chat"), None).await?;
    let bob_conv = conv_repo.create(bob.id, Some("Bob Chat"), None).await?;

    insert_message(&msg_repo, alice_conv.id, "quantum computing breakthroughs").await?;
    insert_message(&msg_repo, bob_conv.id, "quantum physics homework").await?;

    // Alice searches — should only see her own message.
    let alice_results = msg_repo
        .search_by_user(alice.id, "quantum", None, 10)
        .await?;

    assert!(!alice_results.is_empty(), "alice should find her message");
    for hit in &alice_results {
        assert_eq!(
            hit.conversation_id, alice_conv.id,
            "alice must not see bob's messages"
        );
    }

    // Bob searches — should only see his own message.
    let bob_results = msg_repo.search_by_user(bob.id, "quantum", None, 10).await?;

    assert!(!bob_results.is_empty(), "bob should find his message");
    for hit in &bob_results {
        assert_eq!(
            hit.conversation_id, bob_conv.id,
            "bob must not see alice's messages"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn search_by_user_empty_results(pool: PgPool) -> Result<(), sober_core::error::AppError> {
    let user_repo = PgUserRepo::new(pool.clone());
    let conv_repo = PgConversationRepo::new(pool.clone());
    let msg_repo = PgMessageRepo::new(pool);

    let user = create_test_user(&user_repo, "empty@example.com", "emptyuser").await?;
    let conv = conv_repo.create(user.id, None, None).await?;

    insert_message(&msg_repo, conv.id, "Rust borrow checker discussion").await?;

    let results = msg_repo
        .search_by_user(user.id, "xylophone", None, 10)
        .await?;

    assert!(results.is_empty(), "expected no hits for unrelated term");

    Ok(())
}
