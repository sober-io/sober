# 020 — Secrets Management: Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add envelope encryption and a general-purpose encrypted secret store
scoped to users and groups, with LLM provider key resolution.

**Architecture:** Extend `sober-crypto` with AES-256-GCM envelope encryption
(MEK/DEK), add `SecretRepo` trait to `sober-core`, implement `PgSecretRepo` in
`sober-db`, add LLM key resolution service to `sober-llm`.

**Tech Stack:** `aws-lc-rs` (AES-256-GCM), `sqlx` (PostgreSQL), `serde_json` (secret blobs)

---

## Prerequisites

- Plan 003 (sober-core) must be complete — provides `AppError`, `UserId`, `GroupId`, `AppConfig`
- Plan 004 (sober-crypto) must be complete — provides `CryptoError`, module structure
- Plan 005 (sober-db) must be complete — provides `PgPool`, `create_pool()`, repo pattern

---

## Steps

### Task 1: Add AES-256-GCM dependencies to sober-crypto

**Files:**
- Modify: `backend/crates/sober-crypto/Cargo.toml`

**Step 1: Add dependencies**

Add to `Cargo.toml`:

```toml
[dependencies]
aes-gcm = "0.10"
rand = { workspace = true }
```

Note: `aes-gcm` uses `aes` which can use `aws-lc-rs` as a backend. Check that
the crate resolves correctly with the workspace crypto backend.

**Step 2: Verify it compiles**

Run: `cargo check -p sober-crypto`
Expected: compiles without error

**Step 3: Commit**

```
feat(crypto): add aes-gcm dependency for envelope encryption
```

---

### Task 2: Implement EncryptedBlob type

**Files:**
- Create: `backend/crates/sober-crypto/src/envelope.rs`
- Modify: `backend/crates/sober-crypto/src/lib.rs`

**Step 1: Write the test**

In `backend/crates/sober-crypto/src/envelope.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_blob_roundtrip_serialization() {
        let blob = EncryptedBlob {
            nonce: [1u8; 12],
            ciphertext: vec![2, 3, 4, 5],
        };
        let bytes = blob.to_bytes();
        let recovered = EncryptedBlob::from_bytes(&bytes).unwrap();
        assert_eq!(blob.nonce, recovered.nonce);
        assert_eq!(blob.ciphertext, recovered.ciphertext);
    }

    #[test]
    fn encrypted_blob_from_bytes_too_short() {
        let bytes = vec![0u8; 11]; // less than 12 bytes for nonce
        assert!(EncryptedBlob::from_bytes(&bytes).is_err());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sober-crypto -- encrypted_blob`
Expected: FAIL — `EncryptedBlob` not defined

**Step 3: Implement EncryptedBlob**

```rust
use crate::error::CryptoError;

/// AES-256-GCM encrypted payload: 12-byte nonce followed by ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedBlob {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    /// Serialize to bytes: nonce (12 bytes) || ciphertext.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12 + self.ciphertext.len());
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.ciphertext);
        buf
    }

    /// Deserialize from bytes: first 12 bytes are nonce, rest is ciphertext.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() < 12 {
            return Err(CryptoError::InvalidData("encrypted blob too short".into()));
        }
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[..12]);
        let ciphertext = bytes[12..].to_vec();
        Ok(Self { nonce, ciphertext })
    }
}
```

Add `InvalidData(String)` variant to `CryptoError` if not already present.

**Step 4: Register module in lib.rs**

Add `pub mod envelope;` to `backend/crates/sober-crypto/src/lib.rs`.

**Step 5: Run tests to verify they pass**

Run: `cargo test -p sober-crypto -- encrypted_blob`
Expected: PASS (2 tests)

**Step 6: Commit**

```
feat(crypto): add EncryptedBlob type with byte serialization
```

---

### Task 3: Implement Dek (Data Encryption Key)

**Files:**
- Modify: `backend/crates/sober-crypto/src/envelope.rs`

**Step 1: Write the tests**

```rust
#[test]
fn dek_encrypt_decrypt_roundtrip() {
    let dek = Dek::generate().unwrap();
    let plaintext = b"hello world, this is a secret";
    let blob = dek.encrypt(plaintext).unwrap();
    let decrypted = dek.decrypt(&blob).unwrap();
    assert_eq!(plaintext.as_slice(), &decrypted);
}

#[test]
fn dek_decrypt_with_wrong_key_fails() {
    let dek1 = Dek::generate().unwrap();
    let dek2 = Dek::generate().unwrap();
    let blob = dek1.encrypt(b"secret").unwrap();
    assert!(dek2.decrypt(&blob).is_err());
}

#[test]
fn dek_encrypt_produces_different_ciphertexts() {
    let dek = Dek::generate().unwrap();
    let plaintext = b"same input";
    let blob1 = dek.encrypt(plaintext).unwrap();
    let blob2 = dek.encrypt(plaintext).unwrap();
    // Different nonces should produce different ciphertexts
    assert_ne!(blob1.ciphertext, blob2.ciphertext);
}

#[test]
fn dek_encrypts_empty_plaintext() {
    let dek = Dek::generate().unwrap();
    let blob = dek.encrypt(b"").unwrap();
    let decrypted = dek.decrypt(&blob).unwrap();
    assert!(decrypted.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-crypto -- dek_`
Expected: FAIL — `Dek` not defined

**Step 3: Implement Dek**

```rust
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use rand::RngCore;

/// Data Encryption Key — encrypts user/group secrets.
pub struct Dek([u8; 32]);

impl Dek {
    /// Generate a new random DEK using OS CSPRNG.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Ok(Self(key))
    }

    /// Create a DEK from raw bytes (e.g., after unwrapping with MEK).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData(
                format!("DEK must be 32 bytes, got {}", bytes.len()),
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Returns the raw key bytes (for wrapping by MEK).
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Encrypt plaintext with a fresh random nonce.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedBlob, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| CryptoError::EncryptionError(e.to_string()))?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::EncryptionError(e.to_string()))?;

        Ok(EncryptedBlob {
            nonce: nonce_bytes,
            ciphertext,
        })
    }

    /// Decrypt ciphertext using the nonce from the blob.
    pub fn decrypt(&self, blob: &EncryptedBlob) -> Result<Vec<u8>, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| CryptoError::DecryptionError(e.to_string()))?;

        let nonce = Nonce::from_slice(&blob.nonce);

        cipher
            .decrypt(nonce, blob.ciphertext.as_slice())
            .map_err(|e| CryptoError::DecryptionError(e.to_string()))
    }
}

impl Drop for Dek {
    fn drop(&mut self) {
        // Zero out key material on drop
        self.0.fill(0);
    }
}
```

Add `EncryptionError(String)` and `DecryptionError(String)` variants to `CryptoError`
if not already present.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-crypto -- dek_`
Expected: PASS (4 tests)

**Step 5: Commit**

```
feat(crypto): implement Dek with AES-256-GCM encrypt/decrypt
```

---

### Task 4: Implement Mek (Master Encryption Key)

**Files:**
- Modify: `backend/crates/sober-crypto/src/envelope.rs`

**Step 1: Write the tests**

```rust
#[test]
fn mek_wrap_unwrap_dek_roundtrip() {
    let mek = Mek::from_hex(
        &"ab".repeat(32) // 64 hex chars = 32 bytes
    ).unwrap();
    let dek = Dek::generate().unwrap();
    let wrapped = mek.wrap_dek(&dek).unwrap();
    let unwrapped = mek.unwrap_dek(&wrapped).unwrap();
    assert_eq!(dek.as_bytes(), unwrapped.as_bytes());
}

#[test]
fn mek_unwrap_with_wrong_key_fails() {
    let mek1 = Mek::from_hex(&"ab".repeat(32)).unwrap();
    let mek2 = Mek::from_hex(&"cd".repeat(32)).unwrap();
    let dek = Dek::generate().unwrap();
    let wrapped = mek1.wrap_dek(&dek).unwrap();
    assert!(mek2.unwrap_dek(&wrapped).is_err());
}

#[test]
fn mek_from_hex_invalid_length() {
    assert!(Mek::from_hex("abcd").is_err()); // too short
}

#[test]
fn mek_from_hex_invalid_chars() {
    assert!(Mek::from_hex(&"zz".repeat(32)).is_err()); // not hex
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-crypto -- mek_`
Expected: FAIL — `Mek` not defined

**Step 3: Implement Mek**

```rust
/// Master Encryption Key — wraps/unwraps DEKs. Loaded from env at startup.
pub struct Mek([u8; 32]);

impl Mek {
    /// Parse from a 64-character hex string (e.g., from env var).
    pub fn from_hex(hex: &str) -> Result<Self, CryptoError> {
        let bytes = hex::decode(hex)
            .map_err(|e| CryptoError::InvalidData(format!("invalid hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData(
                format!("MEK must be 32 bytes, got {}", bytes.len()),
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    /// Wrap (encrypt) a DEK for storage in the database.
    pub fn wrap_dek(&self, dek: &Dek) -> Result<EncryptedBlob, CryptoError> {
        // Reuse the same AES-256-GCM logic — MEK encrypts DEK bytes
        let wrapper = Dek(self.0);
        wrapper.encrypt(dek.as_bytes())
    }

    /// Unwrap (decrypt) a DEK from its stored form.
    pub fn unwrap_dek(&self, blob: &EncryptedBlob) -> Result<Dek, CryptoError> {
        let wrapper = Dek(self.0);
        let raw = wrapper.decrypt(blob)?;
        Dek::from_bytes(&raw)
    }
}

impl Drop for Mek {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
```

Add `hex` crate to `sober-crypto` dependencies.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-crypto -- mek_`
Expected: PASS (4 tests)

**Step 5: Commit**

```
feat(crypto): implement Mek for DEK wrapping/unwrapping
```

---

### Task 5: Property-based tests for envelope encryption

**Files:**
- Modify: `backend/crates/sober-crypto/src/envelope.rs`

**Step 1: Add proptest dependency**

Add to `sober-crypto/Cargo.toml`:

```toml
[dev-dependencies]
proptest = "1"
```

**Step 2: Write property tests**

```rust
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn dek_encrypt_decrypt_any_data(data in prop::collection::vec(any::<u8>(), 0..4096)) {
            let dek = Dek::generate().unwrap();
            let blob = dek.encrypt(&data).unwrap();
            let decrypted = dek.decrypt(&blob).unwrap();
            prop_assert_eq!(data, decrypted);
        }

        #[test]
        fn encrypted_blob_bytes_roundtrip(
            nonce in prop::array::uniform12(any::<u8>()),
            ciphertext in prop::collection::vec(any::<u8>(), 0..1024),
        ) {
            let blob = EncryptedBlob { nonce, ciphertext: ciphertext.clone() };
            let bytes = blob.to_bytes();
            let recovered = EncryptedBlob::from_bytes(&bytes).unwrap();
            prop_assert_eq!(nonce, recovered.nonce);
            prop_assert_eq!(ciphertext, recovered.ciphertext);
        }

        #[test]
        fn mek_wrap_unwrap_any_dek(mek_bytes in prop::array::uniform32(any::<u8>())) {
            let mek = Mek(mek_bytes);
            let dek = Dek::generate().unwrap();
            let wrapped = mek.wrap_dek(&dek).unwrap();
            let unwrapped = mek.unwrap_dek(&wrapped).unwrap();
            prop_assert_eq!(dek.as_bytes(), unwrapped.as_bytes());
        }
    }
}
```

**Step 3: Run property tests**

Run: `cargo test -p sober-crypto -- proptests`
Expected: PASS (256 cases each)

**Step 4: Commit**

```
test(crypto): add property-based tests for envelope encryption
```

---

### Task 6: Add SecretScope and SecretRepo trait to sober-core

**Files:**
- Create: `backend/crates/sober-core/src/repo/secret.rs`
- Modify: `backend/crates/sober-core/src/repo/mod.rs` (or wherever repo traits live)

**Step 1: Define types and trait**

```rust
use crate::{AppError, UserId, GroupId};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Determines whether a secret is scoped to a user or a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretScope {
    User(UserId),
    Group(GroupId),
}

/// Metadata returned when listing secrets (no encrypted data).
#[derive(Debug, Clone, Serialize)]
pub struct SecretMetadata {
    pub id: Uuid,
    pub name: String,
    pub secret_type: String,
    pub metadata: serde_json::Value,
    pub priority: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Full secret row including encrypted data.
#[derive(Debug, Clone)]
pub struct SecretRow {
    pub id: Uuid,
    pub user_id: Option<UserId>,
    pub group_id: Option<GroupId>,
    pub name: String,
    pub secret_type: String,
    pub metadata: serde_json::Value,
    pub encrypted_data: Vec<u8>,
    pub priority: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new secret.
#[derive(Debug)]
pub struct NewSecret {
    pub scope: SecretScope,
    pub name: String,
    pub secret_type: String,
    pub metadata: serde_json::Value,
    pub encrypted_data: Vec<u8>,
    pub priority: Option<i32>,
}

/// Input for updating an existing secret.
#[derive(Debug)]
pub struct UpdateSecret {
    pub name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub encrypted_data: Option<Vec<u8>>,
    pub priority: Option<Option<i32>>,
}

/// Stored DEK for a user or group.
#[derive(Debug, Clone)]
pub struct StoredDek {
    pub id: Uuid,
    pub scope: SecretScope,
    pub encrypted_dek: Vec<u8>,
    pub mek_version: i32,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
}

#[async_trait::async_trait]
pub trait SecretRepo: Send + Sync {
    /// Get the encrypted DEK for a scope, if one exists.
    async fn get_dek(&self, scope: SecretScope) -> Result<Option<StoredDek>, AppError>;

    /// Store or replace the encrypted DEK for a scope.
    async fn store_dek(
        &self,
        scope: SecretScope,
        encrypted_dek: Vec<u8>,
        mek_version: i32,
    ) -> Result<(), AppError>;

    /// List secret metadata (without encrypted data) for a scope.
    async fn list_secrets(
        &self,
        scope: SecretScope,
        secret_type: Option<&str>,
    ) -> Result<Vec<SecretMetadata>, AppError>;

    /// Get a single secret including encrypted data.
    async fn get_secret(&self, id: Uuid) -> Result<Option<SecretRow>, AppError>;

    /// Store a new secret. Returns the generated ID.
    async fn store_secret(&self, secret: NewSecret) -> Result<Uuid, AppError>;

    /// Update an existing secret.
    async fn update_secret(&self, id: Uuid, update: UpdateSecret) -> Result<(), AppError>;

    /// Delete a secret by ID.
    async fn delete_secret(&self, id: Uuid) -> Result<(), AppError>;
}
```

**Step 2: Register module and re-export**

Add `pub mod secret;` to the repo module, re-export types from `sober-core::repo`.

**Step 3: Verify it compiles**

Run: `cargo check -p sober-core`
Expected: compiles

**Step 4: Commit**

```
feat(core): add SecretRepo trait and secret domain types
```

---

### Task 7: Add database migration for encryption_keys and user_secrets

**Files:**
- Create: `backend/migrations/YYYYMMDDHHMMSS_create_encryption_keys.sql`
- Create: `backend/migrations/YYYYMMDDHHMMSS_create_user_secrets.sql`

**Step 1: Create encryption_keys migration**

```sql
CREATE TABLE encryption_keys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
    group_id      UUID REFERENCES groups(id) ON DELETE CASCADE,
    encrypted_dek BYTEA NOT NULL,
    mek_version   INT NOT NULL DEFAULT 1,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    rotated_at    TIMESTAMPTZ,
    CONSTRAINT encryption_keys_one_scope CHECK (
        (user_id IS NOT NULL AND group_id IS NULL) OR
        (user_id IS NULL AND group_id IS NOT NULL)
    )
);

CREATE UNIQUE INDEX idx_encryption_keys_user ON encryption_keys(user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX idx_encryption_keys_group ON encryption_keys(group_id) WHERE group_id IS NOT NULL;
```

**Step 2: Create user_secrets migration**

```sql
CREATE TABLE user_secrets (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID REFERENCES users(id) ON DELETE CASCADE,
    group_id       UUID REFERENCES groups(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    secret_type    TEXT NOT NULL,
    metadata       JSONB NOT NULL DEFAULT '{}',
    encrypted_data BYTEA NOT NULL,
    priority       INT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT user_secrets_one_scope CHECK (
        (user_id IS NOT NULL AND group_id IS NULL) OR
        (user_id IS NULL AND group_id IS NOT NULL)
    )
);

CREATE INDEX idx_user_secrets_user ON user_secrets(user_id) WHERE user_id IS NOT NULL;
CREATE INDEX idx_user_secrets_group ON user_secrets(group_id) WHERE group_id IS NOT NULL;
CREATE INDEX idx_user_secrets_type ON user_secrets(secret_type);
```

**Step 3: Run migrations**

Run: `sqlx migrate run --source backend/migrations`
Expected: migrations apply successfully

**Step 4: Commit**

```
feat(db): add encryption_keys and user_secrets tables
```

---

### Task 8: Implement PgSecretRepo in sober-db

**Files:**
- Create: `backend/crates/sober-db/src/secret.rs`
- Modify: `backend/crates/sober-db/src/lib.rs`

**Step 1: Write integration tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::repo::secret::*;

    // Assumes test_db() helper from sober-core test-utils

    #[sqlx::test]
    async fn store_and_get_dek(pool: PgPool) {
        let repo = PgSecretRepo::new(pool);
        let scope = SecretScope::User(test_user_id());

        // Initially no DEK
        assert!(repo.get_dek(scope).await.unwrap().is_none());

        // Store a DEK
        repo.store_dek(scope, vec![1, 2, 3], 1).await.unwrap();
        let dek = repo.get_dek(scope).await.unwrap().unwrap();
        assert_eq!(dek.encrypted_dek, vec![1, 2, 3]);
        assert_eq!(dek.mek_version, 1);
    }

    #[sqlx::test]
    async fn crud_secrets(pool: PgPool) {
        let repo = PgSecretRepo::new(pool);
        let scope = SecretScope::User(test_user_id());

        // Create
        let id = repo.store_secret(NewSecret {
            scope,
            name: "Test Key".into(),
            secret_type: "llm_provider".into(),
            metadata: serde_json::json!({"provider": "anthropic"}),
            encrypted_data: vec![10, 20, 30],
            priority: Some(1),
        }).await.unwrap();

        // Read
        let secret = repo.get_secret(id).await.unwrap().unwrap();
        assert_eq!(secret.name, "Test Key");
        assert_eq!(secret.encrypted_data, vec![10, 20, 30]);

        // List
        let list = repo.list_secrets(scope, Some("llm_provider")).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Test Key");

        // Update
        repo.update_secret(id, UpdateSecret {
            name: Some("Updated Key".into()),
            metadata: None,
            encrypted_data: None,
            priority: None,
        }).await.unwrap();
        let updated = repo.get_secret(id).await.unwrap().unwrap();
        assert_eq!(updated.name, "Updated Key");

        // Delete
        repo.delete_secret(id).await.unwrap();
        assert!(repo.get_secret(id).await.unwrap().is_none());
    }

    #[sqlx::test]
    async fn list_secrets_filters_by_type(pool: PgPool) {
        let repo = PgSecretRepo::new(pool);
        let scope = SecretScope::User(test_user_id());

        repo.store_secret(NewSecret {
            scope,
            name: "LLM Key".into(),
            secret_type: "llm_provider".into(),
            metadata: serde_json::json!({}),
            encrypted_data: vec![1],
            priority: Some(1),
        }).await.unwrap();

        repo.store_secret(NewSecret {
            scope,
            name: "OAuth App".into(),
            secret_type: "oauth_app".into(),
            metadata: serde_json::json!({}),
            encrypted_data: vec![2],
            priority: None,
        }).await.unwrap();

        let llm_only = repo.list_secrets(scope, Some("llm_provider")).await.unwrap();
        assert_eq!(llm_only.len(), 1);

        let all = repo.list_secrets(scope, None).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[sqlx::test]
    async fn list_secrets_ordered_by_priority(pool: PgPool) {
        let repo = PgSecretRepo::new(pool);
        let scope = SecretScope::User(test_user_id());

        repo.store_secret(NewSecret {
            scope,
            name: "Fallback".into(),
            secret_type: "llm_provider".into(),
            metadata: serde_json::json!({}),
            encrypted_data: vec![1],
            priority: Some(2),
        }).await.unwrap();

        repo.store_secret(NewSecret {
            scope,
            name: "Primary".into(),
            secret_type: "llm_provider".into(),
            metadata: serde_json::json!({}),
            encrypted_data: vec![2],
            priority: Some(1),
        }).await.unwrap();

        let list = repo.list_secrets(scope, Some("llm_provider")).await.unwrap();
        assert_eq!(list[0].name, "Primary");
        assert_eq!(list[1].name, "Fallback");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sober-db -- secret`
Expected: FAIL — `PgSecretRepo` not defined

**Step 3: Implement PgSecretRepo**

```rust
use sqlx::PgPool;
use sober_core::repo::secret::*;
use sober_core::{AppError, UserId, GroupId};
use uuid::Uuid;

pub struct PgSecretRepo {
    pool: PgPool,
}

impl PgSecretRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl SecretRepo for PgSecretRepo {
    async fn get_dek(&self, scope: SecretScope) -> Result<Option<StoredDek>, AppError> {
        let row = match scope {
            SecretScope::User(uid) => {
                sqlx::query_as!(StoredDekRow,
                    "SELECT * FROM encryption_keys WHERE user_id = $1",
                    uid.as_uuid()
                )
                .fetch_optional(&self.pool)
                .await
            }
            SecretScope::Group(gid) => {
                sqlx::query_as!(StoredDekRow,
                    "SELECT * FROM encryption_keys WHERE group_id = $1",
                    gid.as_uuid()
                )
                .fetch_optional(&self.pool)
                .await
            }
        }
        .map_err(|e| AppError::Internal(e.into()))?;

        Ok(row.map(|r| r.into_domain(scope)))
    }

    async fn store_dek(
        &self,
        scope: SecretScope,
        encrypted_dek: Vec<u8>,
        mek_version: i32,
    ) -> Result<(), AppError> {
        let (user_id, group_id) = scope.to_ids();
        sqlx::query!(
            "INSERT INTO encryption_keys (user_id, group_id, encrypted_dek, mek_version)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (COALESCE(user_id, '00000000-0000-0000-0000-000000000000'),
                          COALESCE(group_id, '00000000-0000-0000-0000-000000000000'))
             DO UPDATE SET encrypted_dek = $3, mek_version = $4, rotated_at = now()",
            user_id,
            group_id,
            &encrypted_dek,
            mek_version,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }

    // ... remaining methods follow the same pattern
    // list_secrets: SELECT with optional secret_type filter, ORDER BY priority NULLS LAST
    // get_secret: SELECT by id
    // store_secret: INSERT with scope decomposed to user_id/group_id
    // update_secret: UPDATE with optional fields
    // delete_secret: DELETE by id
}
```

Note: the `store_dek` upsert uses the unique partial indexes. Adjust the ON CONFLICT
clause to use the actual unique indexes (`idx_encryption_keys_user`, `idx_encryption_keys_group`)
or use separate INSERT/UPDATE logic per scope type.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sober-db -- secret`
Expected: PASS

**Step 5: Commit**

```
feat(db): implement PgSecretRepo for encrypted secret storage
```

---

### Task 9: Add MASTER_ENCRYPTION_KEY to AppConfig

**Files:**
- Modify: `backend/crates/sober-core/src/config.rs`

**Step 1: Add config field**

Add to `AppConfig` (or a new `CryptoConfig` section):

```rust
pub struct CryptoConfig {
    /// Hex-encoded 256-bit master encryption key for envelope encryption.
    /// Required if secrets management is enabled.
    pub master_encryption_key: Option<String>,
}
```

Load from `MASTER_ENCRYPTION_KEY` env var. Optional — system works without it
(secrets features disabled), but `Mek::from_hex` will fail if someone tries to
store secrets without it configured.

**Step 2: Add to .env.example**

```
# Encryption (secrets management)
# Generate with: openssl rand -hex 32
# MASTER_ENCRYPTION_KEY=
```

**Step 3: Verify it compiles**

Run: `cargo check -p sober-core`

**Step 4: Commit**

```
feat(core): add MASTER_ENCRYPTION_KEY to config
```

---

### Task 10: Integration test — full envelope encryption roundtrip

**Files:**
- Create: `backend/crates/sober-crypto/tests/envelope_integration.rs`

**Step 1: Write the integration test**

```rust
use sober_crypto::envelope::{Mek, Dek, EncryptedBlob};

#[test]
fn full_envelope_encryption_roundtrip() {
    // Simulate: system startup loads MEK
    let mek = Mek::from_hex(
        &"deadbeef".repeat(8) // 64 hex chars = 32 bytes
    ).unwrap();

    // Simulate: new user created, generate DEK
    let user_dek = Dek::generate().unwrap();

    // Simulate: wrap DEK for storage in DB
    let wrapped_dek = mek.wrap_dek(&user_dek).unwrap();
    let wrapped_bytes = wrapped_dek.to_bytes(); // what goes into DB

    // Simulate: user stores a secret
    let secret_json = serde_json::json!({"api_key": "sk-ant-1234"});
    let plaintext = serde_json::to_vec(&secret_json).unwrap();
    let encrypted_secret = user_dek.encrypt(&plaintext).unwrap();
    let encrypted_bytes = encrypted_secret.to_bytes(); // what goes into DB

    // Simulate: later, load and decrypt the secret
    let loaded_wrapped_dek = EncryptedBlob::from_bytes(&wrapped_bytes).unwrap();
    let loaded_dek = mek.unwrap_dek(&loaded_wrapped_dek).unwrap();

    let loaded_secret = EncryptedBlob::from_bytes(&encrypted_bytes).unwrap();
    let decrypted = loaded_dek.decrypt(&loaded_secret).unwrap();

    let recovered: serde_json::Value = serde_json::from_slice(&decrypted).unwrap();
    assert_eq!(recovered["api_key"], "sk-ant-1234");
}

#[test]
fn dek_rotation_preserves_secrets() {
    let mek = Mek::from_hex(&"aa".repeat(32)).unwrap();

    // Old DEK with existing secrets
    let old_dek = Dek::generate().unwrap();
    let secret = old_dek.encrypt(b"my secret data").unwrap();

    // Rotate: decrypt with old, re-encrypt with new
    let plaintext = old_dek.decrypt(&secret).unwrap();
    let new_dek = Dek::generate().unwrap();
    let re_encrypted = new_dek.encrypt(&plaintext).unwrap();

    // Verify new DEK can decrypt
    let result = new_dek.decrypt(&re_encrypted).unwrap();
    assert_eq!(result, b"my secret data");

    // Old DEK can't decrypt new ciphertext
    assert!(old_dek.decrypt(&re_encrypted).is_err());

    // Wrap new DEK with MEK
    let wrapped = mek.wrap_dek(&new_dek).unwrap();
    let unwrapped = mek.unwrap_dek(&wrapped).unwrap();
    let final_result = unwrapped.decrypt(&re_encrypted).unwrap();
    assert_eq!(final_result, b"my secret data");
}
```

**Step 2: Run integration tests**

Run: `cargo test -p sober-crypto --test envelope_integration`
Expected: PASS

**Step 3: Commit**

```
test(crypto): add full envelope encryption integration tests
```

---

### Task 11: Add LLM key resolution service to sober-llm

**Files:**
- Create: `backend/crates/sober-llm/src/resolver.rs`
- Modify: `backend/crates/sober-llm/src/lib.rs`

This task adds the three-tier key resolution logic. It depends on `SecretRepo`
from `sober-core` and `Mek`/`Dek`/`EncryptedBlob` from `sober-crypto`.

**Step 1: Add sober-crypto dependency to sober-llm**

Add to `sober-llm/Cargo.toml`:

```toml
sober-crypto = { path = "../sober-crypto" }
```

**Step 2: Write the tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Mock SecretRepo for testing
    struct MockSecretRepo {
        deks: HashMap<SecretScope, StoredDek>,
        secrets: Vec<SecretRow>,
    }

    // ... implement SecretRepo for MockSecretRepo ...

    #[tokio::test]
    async fn resolves_user_key_first() {
        let mek = Mek::from_hex(&"aa".repeat(32)).unwrap();
        let user_dek = Dek::generate().unwrap();
        let encrypted_key = user_dek.encrypt(
            serde_json::to_vec(&serde_json::json!({"api_key": "user-key"})).unwrap().as_slice()
        ).unwrap();

        let repo = MockSecretRepo::with_user_secret(
            test_user_id(),
            user_dek,
            &mek,
            "llm_provider",
            serde_json::json!({"provider": "anthropic", "base_url": "https://api.anthropic.com/v1"}),
            encrypted_key,
            1,
        );

        let resolver = LlmKeyResolver::new(Arc::new(repo), mek, default_llm_config());
        let result = resolver.resolve(test_user_id(), &[]).await.unwrap();
        assert_eq!(result.api_key, "user-key");
    }

    #[tokio::test]
    async fn falls_back_to_group_key() {
        // User has no keys, group has a key
        // ... similar setup with group scope ...
    }

    #[tokio::test]
    async fn falls_back_to_system_config() {
        // No user or group keys, system config used
        let repo = MockSecretRepo::empty();
        let config = LlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key: "system-key".into(),
            model: "gpt-4".into(),
            ..Default::default()
        };
        let resolver = LlmKeyResolver::new(Arc::new(repo), mek, config);
        let result = resolver.resolve(test_user_id(), &[]).await.unwrap();
        assert_eq!(result.api_key, "system-key");
    }

    #[tokio::test]
    async fn respects_priority_ordering() {
        // User has two keys, priority 1 and 2
        // Resolver should return priority 1 first
    }
}
```

**Step 3: Implement LlmKeyResolver**

```rust
use std::sync::Arc;
use sober_core::repo::secret::*;
use sober_core::config::LlmConfig;
use sober_core::{AppError, UserId, GroupId};
use sober_crypto::envelope::{Mek, Dek, EncryptedBlob};

pub struct ResolvedLlmKey {
    pub base_url: String,
    pub api_key: String,
    pub provider: String,
}

// Generic over SecretRepo (RPITIT traits are not dyn-compatible)
pub struct LlmKeyResolver<S: SecretRepo> {
    secret_repo: S,
    mek: Mek,
    system_config: LlmConfig,
}

impl<S: SecretRepo> LlmKeyResolver<S> {
    pub fn new(
        secret_repo: S,
        mek: Mek,
        system_config: LlmConfig,
    ) -> Self {
        Self { secret_repo, mek, system_config }
    }

    /// Resolve the best available LLM key for a user.
    /// Tries: user keys -> group keys -> system config.
    pub async fn resolve(
        &self,
        user_id: UserId,
        group_ids: &[GroupId],
    ) -> Result<ResolvedLlmKey, AppError> {
        // 1. Try user's keys (ordered by priority)
        if let Some(key) = self.try_scope(SecretScope::User(user_id)).await? {
            return Ok(key);
        }

        // 2. Try each group's keys
        for gid in group_ids {
            if let Some(key) = self.try_scope(SecretScope::Group(*gid)).await? {
                return Ok(key);
            }
        }

        // 3. Fall back to system config
        Ok(ResolvedLlmKey {
            base_url: self.system_config.base_url.clone(),
            api_key: self.system_config.api_key.clone(),
            provider: "system".into(),
        })
    }

    async fn try_scope(&self, scope: SecretScope) -> Result<Option<ResolvedLlmKey>, AppError> {
        let secrets = self.secret_repo
            .list_secrets(scope, Some("llm_provider"))
            .await?;

        if secrets.is_empty() {
            return Ok(None);
        }

        // Get the DEK for this scope
        let stored_dek = self.secret_repo.get_dek(scope).await?
            .ok_or_else(|| AppError::Internal(
                anyhow::anyhow!("secrets exist but no DEK for scope")
            ))?;

        let dek_blob = EncryptedBlob::from_bytes(&stored_dek.encrypted_dek)
            .map_err(|e| AppError::Internal(e.into()))?;
        let dek = self.mek.unwrap_dek(&dek_blob)
            .map_err(|e| AppError::Internal(e.into()))?;

        // Try each secret by priority
        for meta in &secrets {
            if let Some(row) = self.secret_repo.get_secret(meta.id).await? {
                let secret_blob = EncryptedBlob::from_bytes(&row.encrypted_data)
                    .map_err(|e| AppError::Internal(e.into()))?;
                let plaintext = dek.decrypt(&secret_blob)
                    .map_err(|e| AppError::Internal(e.into()))?;
                let secret_data: serde_json::Value = serde_json::from_slice(&plaintext)
                    .map_err(|e| AppError::Internal(e.into()))?;

                if let Some(api_key) = secret_data.get("api_key").and_then(|v| v.as_str()) {
                    let base_url = row.metadata.get("base_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let provider = row.metadata.get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    return Ok(Some(ResolvedLlmKey {
                        base_url,
                        api_key: api_key.to_string(),
                        provider,
                    }));
                }
            }
        }

        Ok(None)
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p sober-llm -- resolver`
Expected: PASS

**Step 5: Commit**

```
feat(llm): add three-tier LLM key resolution service
```

---

## Acceptance Criteria

- [ ] `cargo test -p sober-crypto` passes — all unit, property, and integration tests
- [ ] `cargo test -p sober-db` passes — PgSecretRepo CRUD tests against PostgreSQL
- [ ] `cargo test -p sober-llm` passes — key resolution with mock repo
- [ ] `cargo clippy -- -D warnings` clean across all modified crates
- [ ] Migrations apply cleanly to a fresh database
- [ ] `EncryptedBlob`, `Dek`, `Mek` types zero key material on drop
- [ ] No `unsafe` code in envelope encryption module
- [ ] `MASTER_ENCRYPTION_KEY` documented in `.env.example`
