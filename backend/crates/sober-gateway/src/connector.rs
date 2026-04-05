//! Platform connection lifecycle — connect/disconnect bridges from the database.

use std::collections::HashMap;
use std::sync::Arc;

use sober_core::error::AppError;
use sober_core::types::{GatewayPlatformRepo, PlatformId, PlatformType};
use sober_db::PgGatewayPlatformRepo;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::bridge::PlatformBridgeRegistry;
use crate::discord::DiscordBridge;
use crate::types::GatewayEvent;

/// Connects all enabled platforms from the database.
///
/// For each enabled platform, loads credentials from the `credentials`
/// column on `gateway_platforms` (plaintext JSONB), creates the appropriate bridge,
/// and registers it in the registry. Errors for individual platforms are logged
/// and skipped rather than aborting the whole startup sequence.
pub async fn connect_platforms(
    db: &PgPool,
    registry: &Arc<PlatformBridgeRegistry>,
    event_tx: &mpsc::Sender<GatewayEvent>,
) -> Result<(), AppError> {
    let platform_repo = PgGatewayPlatformRepo::new(db.clone());
    let platforms = platform_repo.list(true).await?;

    if platforms.is_empty() {
        info!("no enabled platforms to connect");
        return Ok(());
    }

    for platform in platforms {
        match connect_one(
            &platform_repo,
            platform.id,
            platform.platform_type,
            registry,
            event_tx,
        )
        .await
        {
            Ok(()) => {
                metrics::gauge!(
                    "sober_gateway_platform_connections",
                    "platform" => platform.platform_type.to_string(),
                    "status" => "connected",
                )
                .increment(1.0);
                info!(
                    platform_id = %platform.id,
                    platform_type = %platform.platform_type,
                    name = %platform.display_name,
                    "platform connected"
                );
            }
            Err(e) => {
                metrics::gauge!(
                    "sober_gateway_platform_connections",
                    "platform" => platform.platform_type.to_string(),
                    "status" => "disconnected",
                )
                .increment(1.0);
                error!(
                    platform_id = %platform.id,
                    error = %e,
                    "failed to connect platform — skipping"
                );
            }
        }
    }

    Ok(())
}

/// Connects a single platform and registers its bridge.
async fn connect_one(
    platform_repo: &PgGatewayPlatformRepo,
    platform_id: PlatformId,
    platform_type: PlatformType,
    registry: &Arc<PlatformBridgeRegistry>,
    event_tx: &mpsc::Sender<GatewayEvent>,
) -> Result<(), AppError> {
    let credentials = load_credentials(platform_repo, platform_id).await?;

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

/// Loads credentials for a platform from the `credentials` column on
/// `gateway_platforms` (plaintext JSONB).
async fn load_credentials(
    platform_repo: &PgGatewayPlatformRepo,
    platform_id: PlatformId,
) -> Result<HashMap<String, String>, AppError> {
    let json = platform_repo
        .get_credentials(platform_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("no credentials for platform {platform_id}")))?;

    serde_json::from_value(json)
        .map_err(|e| AppError::Internal(format!("invalid credential JSON: {e}").into()))
}
