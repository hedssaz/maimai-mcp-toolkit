use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_render::MusicGlobalStatsRenderer;
use serde_json::json;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
    image_output::{ImageOutputPolicy, ImageOutputStore},
    music_info::{KnownMusicMetadata, MusicInfoRequest},
};

use super::{MusicGlobalStatsDifficulty, MusicGlobalStatsRequest, MusicGlobalStatsService};

#[tokio::test]
async fn real_catalog_dual_variants_keep_order_and_write_atomic_png_files()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let service = service(
        &temp,
        CatalogFiles::from_data_dir(workspace_root().join("data")),
    )
    .await?;
    let result = service
        .render(request("Calamity Fortune")?, fixed_now()?)
        .await?;

    assert!(result.errors.is_empty());
    assert_eq!(result.images.len(), 2);
    assert_eq!(result.images[0].chart_type.label(), "ST");
    assert_eq!(result.images[1].chart_type.label(), "DX");
    for image in result.images {
        assert_eq!((image.width, image.height), (1_000, 800));
        assert!(image.image_path.is_file());
        assert_eq!(
            fs::read(&image.image_path)?.get(..8),
            Some(&b"\x89PNG\r\n\x1a\n"[..])
        );
    }
    Ok(())
}

#[tokio::test]
async fn malformed_one_side_is_a_stable_partial_success() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let stats = temp.path().join("stats.json");
    let valid_dist = (1_u64..=14).collect::<Vec<_>>();
    fs::write(
        &stats,
        serde_json::to_vec(&json!({
            "charts": {
                "641": [{}, {}, {}, {
                    "fit_diff": 14.1,
                    "diff": "14",
                    "dist": valid_dist,
                    "fc_dist": [10, 20, 30, 40, 50]
                }],
                "10641": [{}, {}, {}, {
                    "fit_diff": 13.5,
                    "diff": "13+",
                    "dist": [1, 2, 3],
                    "fc_dist": [10, 20, 30, 40, 50]
                }]
            }
        }))?,
    )?;
    let mut files = CatalogFiles::from_data_dir(workspace_root().join("data"));
    files.chart_stats = Some(stats);
    let service = service(&temp, files).await?;
    let result = service
        .render(request("Calamity Fortune")?, fixed_now()?)
        .await?;

    assert_eq!(result.images.len(), 1);
    assert_eq!(result.images[0].chart_type.label(), "ST");
    assert_eq!(result.errors.len(), 1);
    assert_eq!(
        result.errors[0].chart_type.map(|value| value.label()),
        Some("DX")
    );
    assert_eq!(result.errors[0].message, "全服统计 dist 必须恰好包含 14 项");
    Ok(())
}

async fn service(
    temp: &TempDir,
    files: CatalogFiles,
) -> Result<MusicGlobalStatsService, Box<dyn Error>> {
    let catalog = Arc::new(CatalogStore::load(files).await?);
    let static_root = temp.path().join("static");
    fs::create_dir(&static_root)?;
    let font = workspace_root().join("crates/maimai-render/tests/fixtures/DejaVuSans-ASCII.ttf");
    fs::copy(&font, static_root.join("Torus SemiBold.otf"))?;
    fs::copy(&font, static_root.join("ResourceHanRoundedCN-Bold.ttf"))?;
    Ok(MusicGlobalStatsService::new(
        catalog,
        MusicGlobalStatsRenderer::new(&static_root)?,
        ImageOutputStore::new(temp.path().join("images"), ImageOutputPolicy::standard())?,
    ))
}

fn request(query: &str) -> Result<MusicGlobalStatsRequest, crate::music_info::MusicInfoError> {
    Ok(MusicGlobalStatsRequest {
        music: MusicInfoRequest::new(
            None,
            Some(query.to_owned()),
            None,
            None,
            KnownMusicMetadata::default(),
            None,
        )?,
        difficulty: MusicGlobalStatsDifficulty::Master,
    })
}

fn fixed_now() -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp(1_700_000_000)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
