mod base;
mod legacy;
mod upgrade;

use sqlx::SqlitePool;

use crate::{
    LegacyImportReport, StorageError,
    diving_fish_credentials::initialize_diving_fish_credentials_schema,
    identity::initialize_identity_schema, oauth::initialize_oauth_schema,
    player_cache::initialize_player_cache_schema, rankings::initialize_rankings_schema,
};

pub(crate) async fn initialize(pool: &SqlitePool) -> Result<LegacyImportReport, StorageError> {
    base::initialize_base_schema(pool).await?;
    upgrade::apply_column_upgrades(pool).await?;
    initialize_diving_fish_credentials_schema(pool).await?;
    initialize_identity_schema(pool).await?;
    initialize_oauth_schema(pool).await?;
    initialize_player_cache_schema(pool).await?;
    initialize_rankings_schema(pool).await?;
    upgrade::backfill_achievement_units(pool).await?;
    legacy::import_legacy_records(pool).await
}
