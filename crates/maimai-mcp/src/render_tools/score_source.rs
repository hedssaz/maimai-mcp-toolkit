use maimai_core::ScoreSource;
use serde_json::{Map, Value, json};

pub(super) fn parse(value: &str) -> Option<ScoreSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" | "本地" | "cache" | "缓存" => Some(ScoreSource::Local),
        "sy" | "水鱼" | "diving-fish" | "divingfish" | "waterfish" => {
            Some(ScoreSource::DivingFish)
        }
        "lxns" | "落雪" | "luoxue" => Some(ScoreSource::Lxns),
        _ => None,
    }
}

pub(super) fn insert(fields: &mut Map<String, Value>, source: ScoreSource) {
    fields.insert("caption".to_owned(), json!(caption(source)));
    fields.insert("scoreSource".to_owned(), json!(key(source)));
    fields.insert("scoreSourceLabel".to_owned(), json!(label(source)));
}

pub(super) fn caption(source: ScoreSource) -> String {
    format!(
        "当前数据源：{}\n通常使用水鱼；如果已绑定落雪，可以发送 source lxns 切到落雪。\n如果不想绑定外部查分器，可以发送 source local 切到本地缓存。\n本地缓存需要先通过本机器人完成成绩导入。\n发送 source sy 可切回水鱼。",
        label(source)
    )
}

pub(super) const fn key(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "sy",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local",
        ScoreSource::OfficialCn => "official",
    }
}

pub(super) const fn label(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "水鱼",
        ScoreSource::Lxns => "落雪",
        ScoreSource::Local => "本地缓存",
        ScoreSource::OfficialCn => "官方国服",
    }
}

#[cfg(test)]
mod tests {
    use maimai_core::ScoreSource;

    use super::{caption, key, label, parse};

    #[test]
    fn aliases_and_output_copy_are_centralized() {
        assert_eq!(parse(" 落雪 "), Some(ScoreSource::Lxns));
        assert_eq!(parse("unknown"), None);
        assert_eq!(key(ScoreSource::DivingFish), "sy");
        assert_eq!(label(ScoreSource::Local), "本地缓存");
        assert!(caption(ScoreSource::Lxns).starts_with("当前数据源：落雪\n"));
    }
}
