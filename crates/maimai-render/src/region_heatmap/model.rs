use crate::RenderError;

const MAX_ROWS: usize = 128;
const MAX_PROVINCE_CHARS: usize = 32;
const MAX_TIME_CHARS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegionHeatmapRow {
    province: String,
    play_count: u64,
    first_play_time: Option<String>,
}

impl RegionHeatmapRow {
    pub fn new(
        province: impl Into<String>,
        play_count: u64,
        first_play_time: Option<String>,
    ) -> Result<Self, RenderError> {
        let province = province.into();
        validate_text(
            "region_heatmap.province",
            &province,
            MAX_PROVINCE_CHARS,
            false,
        )?;
        if let Some(value) = &first_play_time {
            validate_text(
                "region_heatmap.first_play_time",
                value,
                MAX_TIME_CHARS,
                true,
            )?;
        }
        Ok(Self {
            province,
            play_count,
            first_play_time,
        })
    }

    pub fn province(&self) -> &str {
        &self.province
    }

    pub const fn play_count(&self) -> u64 {
        self.play_count
    }

    pub fn first_play_time(&self) -> Option<&str> {
        self.first_play_time.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegionHeatmapView {
    rows: Vec<RegionHeatmapRow>,
    status_label: Option<String>,
}

impl RegionHeatmapView {
    pub fn new(
        rows: Vec<RegionHeatmapRow>,
        status_label: Option<String>,
    ) -> Result<Self, RenderError> {
        if rows.len() > MAX_ROWS {
            return Err(RenderError::invalid(
                "region_heatmap.rows",
                "must contain at most 128 rows",
            ));
        }
        if let Some(value) = &status_label {
            validate_text("region_heatmap.status_label", value, 64, true)?;
        }
        Ok(Self { rows, status_label })
    }

    pub fn rows(&self) -> &[RegionHeatmapRow] {
        &self.rows
    }

    pub fn status_label(&self) -> Option<&str> {
        self.status_label.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegionHeatmapRenderedPng {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub callout_regions: Vec<String>,
}

fn validate_text(
    field: &'static str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), RenderError> {
    if (!allow_empty && value.trim().is_empty())
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(RenderError::invalid(
            field,
            "contains invalid or excessive text",
        ));
    }
    Ok(())
}
