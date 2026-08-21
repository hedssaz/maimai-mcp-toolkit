use serde_json::Value;

pub fn text(result: &Value) -> String {
    let song = &result["selectedSong"];
    let selection = &result["selection"];
    let lookup = &result["lookup"];
    let target = lookup["qq"]
        .as_str()
        .or_else(|| lookup["username"].as_str())
        .or_else(|| lookup["target"].as_str())
        .unwrap_or("未知");
    let ids = result["musicIds"]
        .as_array()
        .into_iter()
        .flatten()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![
        "maimai 单曲成绩查询".to_owned(),
        format!("目标: {target}"),
        format!(
            "曲目: {} / ID: {ids}",
            song["title"]
                .as_str()
                .or_else(|| result["songQuery"].as_str())
                .unwrap_or("未知")
        ),
        format!(
            "成功: {}，失败: {}",
            result["counts"]["success"].as_u64().unwrap_or(0),
            result["counts"]["failure"].as_u64().unwrap_or(0),
        ),
    ];
    if selection["autoSelected"].as_bool() == Some(true) {
        lines.insert(
            3,
            format!(
                "匹配到 {} 首，已自动选择最可能结果（第 {} 个候选）。",
                selection["totalMatches"].as_u64().unwrap_or(0),
                selection["selectedRank"].as_u64().unwrap_or(1),
            ),
        );
    }
    for item in result["scores"].as_array().into_iter().flatten() {
        lines.push(String::new());
        append_item(&mut lines, item);
    }
    lines.join("\n")
}

fn append_item(lines: &mut Vec<String>, item: &Value) {
    let music_id = &item["musicId"];
    if item["ok"].as_bool() != Some(true) {
        lines.push(format!(
            "music_id {music_id}: 查询失败 - {}",
            item["error"]["message"].as_str().unwrap_or("未知错误")
        ));
        return;
    }
    let score = &item["result"];
    let records = score["records"].as_array();
    lines.push(format!(
        "music_id {music_id}: 返回 {} 条成绩",
        records.map_or(0, Vec::len)
    ));
    let mut player = Vec::new();
    if let Some(value) = score["player"]["nickname"].as_str() {
        player.push(format!("昵称: {value}"));
    }
    if !score["player"]["rating"].is_null() {
        player.push(format!("Rating: {}", score["player"]["rating"]));
    }
    if let Some(value) = score["player"]["plate"].as_str() {
        player.push(format!("牌子: {value}"));
    }
    if !player.is_empty() {
        lines.push(player.join(" / "));
    }
    let Some(records) = records else {
        lines.push("未返回该曲成绩数据。".to_owned());
        return;
    };
    if records.is_empty() {
        lines.push("未返回该曲成绩数据。".to_owned());
        return;
    }
    for (index, record) in records.iter().enumerate() {
        lines.push(record_line(record, index + 1));
    }
}

fn record_line(record: &Value, index: usize) -> String {
    let details = [
        record["levelLabel"].as_str().map(str::to_owned),
        record["level"].as_str().map(str::to_owned),
        (!record["ds"].is_null()).then(|| format!("定数 {}", decimal(&record["ds"]))),
        (!record["achievements"].is_null())
            .then(|| format!("{}%", decimal(&record["achievements"]))),
        (!record["ra"].is_null()).then(|| format!("ra {}", record["ra"])),
        record["rate"].as_str().map(str::to_uppercase),
        record["fc"].as_str().map(str::to_uppercase),
        record["fs"].as_str().map(str::to_uppercase),
        (!record["dxScore"].is_null()).then(|| format!("DX Score {}", record["dxScore"])),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" / ");
    let suffix = if details.is_empty() {
        String::new()
    } else {
        format!(" - {details}")
    };
    format!(
        "{index}. [{}] {}{suffix}",
        record["type"].as_str().unwrap_or("?"),
        record["title"].as_str().unwrap_or("未知歌曲"),
    )
}

fn decimal(value: &Value) -> String {
    let mut value = value.to_string();
    if value.contains('.') {
        while value.ends_with('0') && !value.ends_with(".0") {
            value.pop();
        }
    }
    value
}
