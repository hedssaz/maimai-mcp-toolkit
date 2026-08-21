use super::dto::B50DataDto;
use maimai_core::ScoreSource;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

pub(super) fn caption(data: &B50DataDto) -> String {
    let used_source = data
        .source_preference
        .as_ref()
        .and_then(|preference| preference.used_source.as_deref())
        .unwrap_or_else(|| {
            if data.source.as_deref() == Some("local-maimai-db") {
                "local"
            } else {
                "sy"
            }
        });
    let mut lines = vec![
        format!("当前数据源：{}", source_label(used_source)),
        "通常使用水鱼；如果已绑定落雪，可以发送 source lxns 切到落雪。".to_owned(),
        "如果不想绑定外部查分器，可以发送 source local 切到本地缓存。".to_owned(),
        "本地缓存需要先通过本机器人完成成绩导入。".to_owned(),
        "发送 source sy 可切回水鱼。".to_owned(),
    ];
    if data.source.as_deref() == Some("local-maimai-db")
        && let Some(updated_at) = data
            .local_b50
            .as_ref()
            .and_then(|local| local.computed_at.as_deref())
            .or(data.requested_at.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        lines.insert(0, format!("成绩更新时间：{}", local_timestamp(updated_at)));
    }
    lines.join("\n")
}

pub(super) fn render_text(
    nickname: Option<&str>,
    rating: Option<u32>,
    image_path: &std::path::Path,
    missing_covers: usize,
) -> String {
    format!(
        "B50 图片已生成。\n玩家: {} / Rating: {}\n图片: {}\n缺失曲绘: {missing_covers}",
        nickname.filter(|value| !value.is_empty()).unwrap_or("未知"),
        rating
            .filter(|value| *value != 0)
            .map_or_else(|| "未知".to_owned(), |value| value.to_string()),
        image_path.display(),
    )
}

pub(super) fn queried_caption(
    used_source: ScoreSource,
    result_source: ScoreSource,
    now: OffsetDateTime,
) -> String {
    let mut lines = vec![
        format!("当前数据源：{}", typed_source_label(used_source)),
        "通常使用水鱼；如果已绑定落雪，可以发送 source lxns 切到落雪。".to_owned(),
        "如果不想绑定外部查分器，可以发送 source local 切到本地缓存。".to_owned(),
        "本地缓存需要先通过本机器人完成成绩导入。".to_owned(),
        "发送 source sy 可切回水鱼。".to_owned(),
    ];
    if result_source == ScoreSource::Local {
        lines.insert(0, format!("成绩更新时间：{}", china_timestamp(now)));
    }
    lines.join("\n")
}

fn source_label(source: &str) -> &'static str {
    match source.trim().to_ascii_lowercase().as_str() {
        "local" | "local-maimai-db" => "本地缓存",
        "lxns" => "落雪",
        _ => "水鱼",
    }
}

const fn typed_source_label(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "水鱼",
        ScoreSource::Lxns => "落雪",
        ScoreSource::Local => "本地缓存",
        ScoreSource::OfficialCn => "官服",
    }
}

fn china_timestamp(value: OffsetDateTime) -> String {
    let Ok(china) = UtcOffset::from_hms(8, 0, 0) else {
        return utc_fallback(value);
    };
    let value = value.to_offset(china);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn utc_fallback(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn local_timestamp(value: &str) -> String {
    let mut normalized = value.trim().to_owned();
    if normalized.as_bytes().get(10) == Some(&b' ') {
        normalized.replace_range(10..11, "T");
    }
    let time_part = normalized
        .split_once('T')
        .map(|(_, time)| time)
        .unwrap_or("");
    if !normalized.ends_with('Z')
        && !time_part
            .char_indices()
            .skip(1)
            .any(|(_, character)| matches!(character, '+' | '-'))
    {
        normalized.push('Z');
    }
    let Ok(parsed) = OffsetDateTime::parse(&normalized, &Rfc3339) else {
        return value.trim().to_owned();
    };
    let Ok(china) = UtcOffset::from_hms(8, 0, 0) else {
        return value.trim().to_owned();
    };
    let parsed = parsed.to_offset(china);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        parsed.year(),
        u8::from(parsed.month()),
        parsed.day(),
        parsed.hour(),
        parsed.minute(),
        parsed.second()
    )
}

#[cfg(test)]
mod tests {
    use super::caption;
    use crate::b50_image_tools::dto::B50DataDto;

    #[test]
    fn local_caption_uses_china_time() -> Result<(), Box<dyn std::error::Error>> {
        let data: B50DataDto = serde_json::from_value(serde_json::json!({
            "source": "local-maimai-db",
            "requestedAt": "2026-01-15T00:00:00Z"
        }))?;
        assert!(caption(&data).starts_with("成绩更新时间：2026-01-15 08:00:00\n"));
        Ok(())
    }
}
