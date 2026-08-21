use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Number;

use crate::CatalogError;

#[derive(Clone, Debug, Default)]
pub(crate) struct CustomPlateDocument {
    pub(crate) plates: BTreeMap<String, Vec<CustomSong>>,
}

#[derive(Clone, Debug)]
pub(crate) enum CustomSong {
    Reference(CustomSongReference),
    Defined(CustomSongDefinition),
}

#[derive(Clone, Debug)]
pub(crate) struct CustomSongReference {
    pub(crate) query: String,
    pub(crate) generation: Option<CustomGeneration>,
    pub(crate) image_name: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CustomSongDefinition {
    pub(crate) id: Option<String>,
    pub(crate) title: String,
    pub(crate) generation: CustomGeneration,
    pub(crate) image_name: Option<String>,
    pub(crate) levels: Vec<String>,
    pub(crate) constants: Vec<Number>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustomGeneration {
    Standard,
    Deluxe,
}

pub(crate) fn parse(source: Option<&str>) -> Result<CustomPlateDocument, CatalogError> {
    let Some(source) = source else {
        return Ok(CustomPlateDocument::default());
    };
    let wire: DocumentWire = serde_json::from_str(source).map_err(|source| CatalogError::Json {
        document: "自定义牌子",
        source,
    })?;
    let plates = wire
        .into_plates()
        .into_iter()
        .map(|(name, definition)| parse_plate(name, definition))
        .collect::<Result<_, _>>()?;
    Ok(CustomPlateDocument { plates })
}

fn parse_plate(
    name: String,
    definition: PlateDefinitionWire,
) -> Result<(String, Vec<CustomSong>), CatalogError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(invalid(&name, "name", "牌子名为空或包含控制字符"));
    }
    let songs = definition
        .into_songs()
        .into_iter()
        .map(|song| parse_song(&name, song))
        .collect::<Result<_, _>>()?;
    Ok((name, songs))
}

fn parse_song(plate: &str, song: SongWire) -> Result<CustomSong, CatalogError> {
    match song {
        SongWire::Number(value) => reference(plate, value.to_string(), None, None),
        SongWire::Text(value) => reference(plate, value, None, None),
        SongWire::Object(value) => {
            let value = *value;
            let generation = value
                .generation
                .as_deref()
                .map(|value| parse_generation(plate, value))
                .transpose()?;
            let id = value.id.map(IdWire::into_string);
            let query = value
                .query
                .or(value.title.clone())
                .or(value.name.clone())
                .or(id.clone())
                .unwrap_or_default();
            let levels = value.levels.or(value.level).unwrap_or_default();
            let constants = value.constants.or(value.ds).unwrap_or_default();
            if levels.is_empty() && constants.is_empty() {
                return reference(plate, query, generation, value.image_name);
            }
            if levels.is_empty() || constants.is_empty() || levels.len() != constants.len() {
                return Err(invalid(
                    plate,
                    "songs",
                    "自定义歌曲的 level 与 ds 必须非空且长度相同",
                ));
            }
            if levels.len() > 5 {
                return Err(invalid(plate, "songs", "自定义歌曲最多包含五个普通难度"));
            }
            let title = value
                .title
                .or(value.name)
                .unwrap_or(query)
                .trim()
                .to_owned();
            if title.is_empty() || title.chars().any(char::is_control) {
                return Err(invalid(plate, "title", "歌曲标题为空或包含控制字符"));
            }
            Ok(CustomSong::Defined(CustomSongDefinition {
                id,
                title,
                generation: generation.unwrap_or(CustomGeneration::Deluxe),
                image_name: clean_optional(plate, "imageName", value.image_name)?,
                levels,
                constants,
            }))
        }
    }
}

fn reference(
    plate: &str,
    query: String,
    generation: Option<CustomGeneration>,
    image_name: Option<String>,
) -> Result<CustomSong, CatalogError> {
    let query = query.trim().to_owned();
    if query.is_empty() || query.chars().any(char::is_control) {
        return Err(invalid(plate, "query", "歌曲查询为空或包含控制字符"));
    }
    Ok(CustomSong::Reference(CustomSongReference {
        query,
        generation,
        image_name: clean_optional(plate, "imageName", image_name)?,
    }))
}

fn parse_generation(plate: &str, value: &str) -> Result<CustomGeneration, CatalogError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sd" | "st" | "std" | "standard" | "标准" | "標準" => Ok(CustomGeneration::Standard),
        "dx" | "deluxe" | "でらっくす" => Ok(CustomGeneration::Deluxe),
        other => Err(invalid(plate, "type", other)),
    }
}

fn clean_optional(
    plate: &str,
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, CatalogError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(plate, field, "包含控制字符"));
    }
    Ok(Some(value))
}

fn invalid(plate: &str, field: &'static str, value: &str) -> CatalogError {
    CatalogError::UnsupportedSourceValue {
        source_name: "自定义牌子",
        song_id: plate.to_owned(),
        field,
        value: value.to_owned(),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DocumentWire {
    Content {
        content: BTreeMap<String, PlateDefinitionWire>,
    },
    Plates {
        plates: BTreeMap<String, PlateDefinitionWire>,
    },
    Direct(BTreeMap<String, PlateDefinitionWire>),
}

impl DocumentWire {
    fn into_plates(self) -> BTreeMap<String, PlateDefinitionWire> {
        match self {
            Self::Content { content } => content,
            Self::Plates { plates } => plates,
            Self::Direct(values) => values,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PlateDefinitionWire {
    Songs(Vec<SongWire>),
    Object(PlateObjectWire),
}

impl PlateDefinitionWire {
    fn into_songs(self) -> Vec<SongWire> {
        match self {
            Self::Songs(values) => values,
            Self::Object(value) => value
                .songs
                .or(value.music)
                .or(value.music_ids)
                .or(value.ids)
                .unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlateObjectWire {
    songs: Option<Vec<SongWire>>,
    music: Option<Vec<SongWire>>,
    #[serde(alias = "music_ids")]
    music_ids: Option<Vec<SongWire>>,
    ids: Option<Vec<SongWire>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SongWire {
    Number(u32),
    Text(String),
    Object(Box<SongObjectWire>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SongObjectWire {
    #[serde(
        alias = "song_id",
        alias = "songId",
        alias = "music_id",
        alias = "musicId",
        alias = "df_id"
    )]
    id: Option<IdWire>,
    query: Option<String>,
    title: Option<String>,
    name: Option<String>,
    #[serde(rename = "type", alias = "songType")]
    generation: Option<String>,
    #[serde(alias = "image_name")]
    image_name: Option<String>,
    #[serde(alias = "level_values")]
    levels: Option<Vec<String>>,
    level: Option<Vec<String>>,
    #[serde(alias = "ds_values")]
    constants: Option<Vec<Number>>,
    ds: Option<Vec<Number>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IdWire {
    Numeric(u32),
    Text(String),
}

impl IdWire {
    fn into_string(self) -> String {
        match self {
            Self::Numeric(value) => value.to_string(),
            Self::Text(value) => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CustomGeneration, CustomSong, parse};

    #[test]
    fn parses_current_and_rich_custom_shapes() -> Result<(), Box<dyn std::error::Error>> {
        let document = parse(Some(
            r#"{"content":{"雪峰":[11231],"自定":{"songs":[{"title":"Song","type":"SD","level":["1","2"],"ds":[1.0,2.0]}]}}}"#,
        ))?;
        assert_eq!(document.plates["雪峰"].len(), 1);
        let CustomSong::Defined(song) = &document.plates["自定"][0] else {
            return Err("expected defined song".into());
        };
        assert_eq!(song.generation, CustomGeneration::Standard);
        assert_eq!(song.levels, ["1", "2"]);
        Ok(())
    }

    #[test]
    fn rejects_unknown_generation_instead_of_guessing() {
        let error = parse(Some(
            r#"{"content":{"X":[{"title":"Song","type":"future","level":["1"],"ds":[1.0]}]}}"#,
        ));
        assert!(error.is_err());
    }
}
