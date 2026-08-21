use serde_json::Value;

pub(super) fn fmt(value: Option<&Value>, fallback: &str) -> String {
    match value {
        None | Some(Value::Null) => fallback.to_owned(),
        Some(Value::String(value)) if value.is_empty() => fallback.to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Some(Value::Number(value)) if value.is_i64() || value.is_u64() => value.to_string(),
        Some(Value::Number(value)) => value
            .as_f64()
            .map_or_else(|| value.to_string(), format_number),
        Some(Value::Array(value)) if value.is_empty() => fallback.to_owned(),
        Some(Value::Array(value)) => format!(
            "[{}]",
            value
                .iter()
                .map(|item| match item {
                    Value::String(text) => format!("'{text}'"),
                    _ => fmt(Some(item), "-"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(Value::Object(value)) if value.is_empty() => fallback.to_owned(),
        Some(value) => value.to_string(),
    }
}

pub(super) fn format_ds(value: Option<&Value>) -> String {
    let Some(value) = value.filter(|value| present(Some(value))) else {
        return "-".to_owned();
    };
    let Some(numeric) = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    else {
        return fmt(Some(value), "-");
    };
    let mut result = format_number(numeric);
    if numeric.fract() == 0.0 {
        result.push_str(".0");
    }
    result
}

pub(super) fn criteria(value: Option<&Value>) -> String {
    let Some(values) = value.and_then(Value::as_object) else {
        return "-".to_owned();
    };
    let result = values
        .iter()
        .filter(|(_, value)| present(Some(value)))
        .map(|(key, value)| format!("{key}={}", fmt(Some(value), "-")))
        .collect::<Vec<_>>()
        .join(", ");
    if result.is_empty() {
        "-".to_owned()
    } else {
        result
    }
}

pub(super) fn regions(value: Option<&Value>) -> String {
    let Some(regions) = value.and_then(Value::as_object) else {
        return "-".to_owned();
    };
    let result = [
        ("jp", "日服"),
        ("intl", "国际服"),
        ("usa", "美服"),
        ("cn", "国服"),
    ]
    .into_iter()
    .filter(|(key, _)| regions.get(*key).and_then(Value::as_bool).unwrap_or(false))
    .map(|(_, label)| label)
    .collect::<Vec<_>>()
    .join("/");
    if result.is_empty() {
        "-".to_owned()
    } else {
        result
    }
}

pub(super) fn chart_type(value: Option<&Value>) -> String {
    let raw = fmt(value, "-");
    match raw.to_ascii_lowercase().as_str() {
        "standard" | "st" | "sd" => "ST".to_owned(),
        "dx" => "DX".to_owned(),
        _ => raw.to_uppercase(),
    }
}

pub(super) fn chart_type_list(value: Option<&Value>) -> String {
    let mut values = array(value)
        .iter()
        .filter(|value| present(Some(value)))
        .map(|value| chart_type(Some(value)))
        .collect::<Vec<_>>();
    values.sort_by_key(|value| type_order(value));
    values.dedup();
    values.join("/")
}

pub(super) fn song_sources(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('+')
        .map(|source| match source.to_ascii_lowercase().as_str() {
            "lxns" => "落雪国服源",
            "official" => "官方本地源",
            "dxdata" => "日服源 (dxrating)",
            "cndivingfish" | "diving-fish" => "水鱼国服源",
            _ => source,
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub(super) fn chart_source(value: Option<&Value>) -> String {
    let raw = fmt(value, "-");
    match raw.to_ascii_lowercase().as_str() {
        "cn" => "落雪国服源".to_owned(),
        "official" => "官方本地源".to_owned(),
        "jp" => "JP".to_owned(),
        "divingfish" => "水鱼国服源".to_owned(),
        _ => raw.to_uppercase(),
    }
}

pub(super) fn source_label(source: &str) -> &'static str {
    match source {
        "cn" => "落雪国服源",
        "official" => "官方本地源",
        "jp" => "JP",
        "divingfish" => "水鱼国服源",
        _ => "-",
    }
}

pub(super) fn present(value: Option<&Value>) -> bool {
    value.is_some_and(|value| {
        !value.is_null()
            && value.as_str() != Some("")
            && !value.as_array().is_some_and(Vec::is_empty)
            && !value.as_object().is_some_and(serde_json::Map::is_empty)
    })
}

pub(super) fn truthy(value: &Value) -> bool {
    present(Some(value)) && value.as_bool() != Some(false) && value.as_i64() != Some(0)
}

pub(super) fn numeric(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| {
                value.as_f64().and_then(|number| {
                    (number.is_finite() && number >= i64::MIN as f64 && number <= i64::MAX as f64)
                        .then_some(number.trunc() as i64)
                })
            })
            .or_else(|| value.as_str()?.parse().ok())
    })
}

pub(super) fn count(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

pub(super) fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

pub(super) fn type_order(value: &str) -> u8 {
    match value {
        "ST" => 0,
        "DX" => 1,
        _ => 9,
    }
}

pub(super) fn source_rank(value: Option<&Value>) -> Option<u8> {
    match value.and_then(Value::as_str) {
        Some("cn") => Some(0),
        Some("official") => Some(1),
        Some("jp") => Some(2),
        Some("divingfish") => Some(3),
        _ => None,
    }
}

fn format_number(value: f64) -> String {
    let value = format!("{value:.4}");
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}
