use std::{collections::BTreeSet, path::PathBuf};

use maimai_core::{ChartGeneration, SongIdValue};

use super::MusicInfoError;

pub const MAX_MUSIC_INFO_BATCH_ITEMS: usize = 50;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MusicInfoChartType {
    Standard,
    Deluxe,
}

impl MusicInfoChartType {
    pub const fn generation(self) -> ChartGeneration {
        match self {
            Self::Standard => ChartGeneration::Standard,
            Self::Deluxe => ChartGeneration::Deluxe,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Standard => "ST",
            Self::Deluxe => "DX",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KnownMusicMetadata {
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub version: String,
    pub bpm: Option<u32>,
    pub is_new: bool,
}

impl KnownMusicMetadata {
    pub fn new(
        title: impl Into<String>,
        artist: impl Into<String>,
        genre: impl Into<String>,
        version: impl Into<String>,
        bpm: Option<u32>,
        is_new: bool,
    ) -> Result<Self, MusicInfoError> {
        Ok(Self {
            title: safe_text(title.into(), "knownTitle")?,
            artist: safe_text(artist.into(), "artist")?,
            genre: safe_text(genre.into(), "genre")?,
            version: safe_text(version.into(), "version")?,
            bpm,
            is_new,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedSongHint {
    pub id: Option<SongIdValue>,
    pub title: Option<String>,
    pub chart_types: BTreeSet<ChartGeneration>,
    pub image_name: Option<String>,
}

impl ResolvedSongHint {
    pub fn new(
        id: Option<SongIdValue>,
        title: Option<String>,
        chart_types: BTreeSet<ChartGeneration>,
        image_name: Option<String>,
    ) -> Result<Self, MusicInfoError> {
        Ok(Self {
            id,
            title: title
                .map(|value| safe_text(value, "resolvedSong.title"))
                .transpose()?
                .filter(|value| !value.is_empty()),
            chart_types,
            image_name: image_name
                .map(|value| safe_text(value, "resolvedSong.image_name"))
                .transpose()?
                .filter(|value| !value.is_empty()),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoRequest {
    pub music_id: Option<SongIdValue>,
    pub query: Option<String>,
    pub chart_type: Option<MusicInfoChartType>,
    pub image_name: Option<String>,
    pub known: KnownMusicMetadata,
    pub resolved: Option<ResolvedSongHint>,
}

impl MusicInfoRequest {
    pub fn new(
        music_id: Option<SongIdValue>,
        query: Option<String>,
        chart_type: Option<MusicInfoChartType>,
        image_name: Option<String>,
        known: KnownMusicMetadata,
        resolved: Option<ResolvedSongHint>,
    ) -> Result<Self, MusicInfoError> {
        let query = query
            .map(|value| safe_text(value, "query"))
            .transpose()?
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let image_name = image_name
            .map(|value| safe_text(value, "image_name"))
            .transpose()?
            .filter(|value| !value.is_empty());
        let resolved_image = resolved
            .as_ref()
            .and_then(|value| value.image_name.as_ref())
            .is_some();
        if music_id.is_none()
            && query.is_none()
            && resolved.is_none()
            && image_name.is_none()
            && !resolved_image
        {
            return Err(MusicInfoError::invalid(
                "需要提供 music_id/id 或 query/title/songQuery",
            ));
        }
        Ok(Self {
            music_id,
            query,
            chart_type,
            image_name,
            known,
            resolved,
        })
    }

    pub fn query_label(&self) -> String {
        self.query
            .clone()
            .or_else(|| self.music_id.as_ref().map(song_id_text))
            .or_else(|| self.resolved.as_ref().and_then(|value| value.title.clone()))
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoImage {
    pub index: usize,
    pub sub_index: usize,
    pub query: String,
    pub music_id: String,
    pub title: String,
    pub chart_type: Option<MusicInfoChartType>,
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MusicInfoItemError {
    pub index: usize,
    pub sub_index: Option<usize>,
    pub query: String,
    pub chart_type: Option<MusicInfoChartType>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MusicInfoBatchResult {
    pub images: Vec<MusicInfoImage>,
    pub errors: Vec<MusicInfoItemError>,
}

pub(crate) fn song_id_text(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}

fn safe_text(value: String, field: &'static str) -> Result<String, MusicInfoError> {
    if value.chars().any(char::is_control) {
        return Err(MusicInfoError::invalid(format!("{field} 不能包含控制字符")));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{KnownMusicMetadata, MusicInfoRequest};

    #[test]
    fn explicit_image_name_can_drive_cover_only_render_without_song_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = MusicInfoRequest::new(
            None,
            None,
            None,
            Some("local-cover".to_owned()),
            KnownMusicMetadata::default(),
            None,
        )?;
        assert_eq!(request.image_name.as_deref(), Some("local-cover"));
        Ok(())
    }
}
