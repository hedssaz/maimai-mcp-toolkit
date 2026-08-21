use std::path::PathBuf;

use thiserror::Error;

use maimai_core::ChartKey;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("读取曲库文件失败：{path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("解析 {document} JSON 失败")]
    Json {
        document: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("{source_name} 歌曲 {song_id} 的 {field} 数值无效：{value}")]
    InvalidNumber {
        source_name: &'static str,
        song_id: String,
        field: &'static str,
        value: String,
    },

    #[error("{source_name} 歌曲 {song_id} 使用了不支持的难度序号：{difficulty}")]
    InvalidDifficulty {
        source_name: &'static str,
        song_id: String,
        difficulty: usize,
    },

    #[error("Diving-Fish 歌曲 ID 不能为空")]
    EmptyDivingFishSongId,

    #[error("dxdata 歌曲 ID 不能为空")]
    EmptyDxDataSongId,

    #[error("dxdata 歌曲 {song_id} 的 {field} 使用了不支持的值：{value}")]
    UnsupportedDxDataValue {
        song_id: String,
        field: &'static str,
        value: String,
    },

    #[error("dxdata 歌曲 {song_id} 的 releaseDate 无效：{value}")]
    InvalidReleaseDate { song_id: String, value: String },

    #[error("简繁转换表必须是一对一字符映射：{from:?} -> {to:?}")]
    InvalidCharacterMapping { from: String, to: String },

    #[error("解析 legacy alias CSV 失败")]
    Csv(#[source] csv::Error),

    #[error("{source_name} 的歌曲 ID 无效：{value:?}")]
    InvalidSourceSongId {
        source_name: &'static str,
        value: String,
    },

    #[error("{source_name} 歌曲 {song_id} 的 {field} 使用了不支持的值：{value}")]
    UnsupportedSourceValue {
        source_name: &'static str,
        song_id: String,
        field: &'static str,
        value: String,
    },

    #[error("{source_name} 歌曲 {song_id}（{title}）对应多个曲库实体：{candidates:?}")]
    AmbiguousSongIdentity {
        source_name: &'static str,
        song_id: String,
        title: String,
        candidates: Vec<usize>,
    },

    #[error("谱面稳定标识 {key:?} 同时指向多个曲库实体")]
    AmbiguousChartIdentity { key: ChartKey },
}
