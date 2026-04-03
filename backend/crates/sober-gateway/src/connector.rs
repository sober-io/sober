//! Platform connection lifecycle — connect/disconnect bridges from the database.

use std::collections::HashMap;
use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::repo::SecretRepo;
use sober_core::types::{GatewayPlatformRepo, PlatformId, PlatformType, UserId};
use sober_crypto::envelope::{EncryptedBlob, Mek};
use sober_db::{PgGatewayPlatformRepo, PgSecretRepo};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::bridge::PlatformBridgeRegistry;
use crate::discord::DiscordBridge;
use crate::types::GatewayEvent;

/// The Sõber bot user UUID — credentials are stored under this user.
const BOT_USER_UUID: &str = "01960000-0000-7000-8000-000000000100";

/// Connects all enabled platforms from the database.
///
/// For each enabled platform, loads credentials from `user_secrets`,
/// creates the appropriate bridge, and registers it in the registry.
/// Errors for individual platforms are logged and skipped rather than
/// aborting the whole startup sequence.
pub async fn connect_platforms(
    db: &PgPool,
    mek: Option<&Mek>,
    registry: &Arc<PlatformBridgeRegistry>,
    event_tx: &mpsc::Sender<GatewayEvent>,
) -> Result<(), AppError> {
    let platform_repo = PgGatewayPlatformRepo::new(db.clone());
    let platforms = platform_repo.list(true).await?;

    if platforms.is_empty() {
        info!("no enabled platforms to connect");
        return Ok(());
    }

    let Some(mek) = mek else {
        warn!("no MEK configured — cannot decrypt platform credentials, skipping connect");
        return Ok(());
    };

    let bot_uuid = Uuid::parse_str(BOT_USER_UUID).expect("BOT_USER_UUID is a valid UUID constant");
    let bot_user_id = UserId::from_uuid(bot_uuid);
    let secret_repo = PgSecretRepo::new(db.clone());

    for platform in platforms {
        match connect_one(
            &secret_repo,
            mek,
            bot_user_id,
            platform.id,
            platform.platform_type,
            registry,
            event_tx,
        )
        .await
        {
            Ok(()) => info!(
                platform_id = %platform.id,
                platform_type = %platform.platform_type,
                name = %platform.display_name,
                "platform connected"
            ),
            Err(e) => error!(
                platform_id = %platform.id,
                error = %e,
                "failed to connect platform — skipping"
            ),
        }
    }

    Ok(())
}

/// Connects a single platform and registers its bridge.
async fn connect_one(
    secret_repo: &PgSecretRepo,
    mek: &Mek,
    bot_user_id: UserId,
    platform_id: PlatformId,
    platform_type: PlatformType,
    registry: &Arc<PlatformBridgeRegistry>,
    event_tx: &mpsc::Sender<GatewayEvent>,
) -> Result<(), AppError> {
    let credentials = load_credentials(secret_repo, mek, bot_user_id, platform_id).await?;

    match platform_type {
        PlatformType::Discord => {
            let bot_token = credentials.get("bot_token").ok_or_else(|| {
                AppError::Validation(
                    "Discord platform credentials missing 'bot_token' field".into(),
                )
            })?;

            let bridge = DiscordBridge::connect(platform_id, bot_token, event_tx.clone())
                .await
                .map_err(|e| AppError::Internal(e.to_string().into()))?;

            registry.insert(platform_id, bridge);
        }
        other => {
            warn!(platform_type = %other, "platform type not yet supported — skipping");
        }
    }

    Ok(())
}

/// Loads and decrypts credentials for a platform from `user_secrets`.
///
/// Looks for a secret of type `gateway_platform` whose `metadata.platform_id`
/// matches the given `platform_id`, then decrypts it with the bot user's DEK.
async fn load_credentials(
    secret_repo: &PgSecretRepo,
    mek: &Mek,
    bot_user_id: UserId,
    platform_id: PlatformId,
) -> Result<HashMap<String, String>, AppError> {
    let secrets = secret_repo
        .list_secrets(bot_user_id, None, Some("gateway_platform"))
        .await?;

    let platform_id_str = platform_id.to_string();

    for meta in secrets {
        if meta.metadata.get("platform_id").and_then(|v| v.as_str()) != Some(&platform_id_str) {
            continue;
        }

        let row = match secret_repo.get_secret(meta.id).await? {
            Some(r) => r,
            None => continue,
        };

        let stored_dek = match secret_repo.get_dek(bot_user_id).await? {
            Some(d) => d,
            None => {
                return Err(AppError::Internal(
                    "no DEK found for bot user — cannot decrypt platform credentials".into(),
                ));
            }
        };

        let dek_blob = EncryptedBlob::from_bytes(&stored_dek.encrypted_dek)?;
        let dek = mek.unwrap_dek(&dek_blob)?;

        let data_blob = EncryptedBlob::from_bytes(&row.encrypted_data)?;
        let plaintext = dek.decrypt(&data_blob)?;

        let kv: HashMap<String, String> = serde_json::from_slice(&plaintext).map_err(|e| {
            AppError::Internal(
                format!("invalid credential JSON for platform {platform_id}: {e}").into(),
            )
        })?;

        return Ok(kv);
    }

    Err(AppError::NotFound(format!(
        "no credentials found for platform {platform_id}"
    )))
}
