use std::{cmp::Reverse, collections::HashMap};

use maimai_catalog::{CatalogSnapshot, PlateMember, PlateServer};
use maimai_core::{ChartConstant, ChartKey, Difficulty};

use crate::scores::{B50Chart, PlayerScores, best_by_chart};

use super::{
    CompletionError, CompletionTarget, PlateSpec,
    plate::{display_id, resolved_members},
    records,
};

pub(crate) struct PreparedPlateProgress {
    pub(crate) version: String,
    pub(crate) target: CompletionTarget,
    pub(crate) server: PlateServer,
    pub(crate) text: String,
    pub(crate) listed_count: usize,
}

struct RemainingChart {
    id: String,
    title: String,
    difficulty: Difficulty,
    constant: Option<ChartConstant>,
    record: Option<String>,
}

pub(crate) fn prepare(
    snapshot: &CatalogSnapshot,
    scores: &PlayerScores,
    spec: &PlateSpec,
    allow_jp: bool,
) -> Result<PreparedPlateProgress, CompletionError> {
    let (server, members) = resolved_members(snapshot, spec, allow_jp)?;
    let records = best_by_chart(&scores.records);
    let mut counts = [0usize; 5];
    let mut remaining = Vec::new();
    for member in members.members() {
        append_member(member, spec.target, &records, &mut counts, &mut remaining)?;
    }
    remaining.sort_by_key(|value| Reverse(value.constant));
    let difficult_threshold = ChartConstant::from_hundredths(1_360)
        .map_err(|_| CompletionError::invalid("内部高定数阈值无效"))?;
    let difficult = remaining
        .iter()
        .filter(|chart| {
            chart
                .constant
                .is_some_and(|value| value > difficult_threshold)
        })
        .collect::<Vec<_>>();
    let username = match &scores.lookup {
        crate::scores::Lookup::Username(value) => value.as_str(),
        crate::scores::Lookup::Qq(_) => "您",
    };
    let version = normalized_version(spec.version.as_str(), server);
    let mut text = format!(
        "{username}的「{}{target}」剩余进度如下：\nBasic剩余「{}」首\nAdvanced剩余「{}」首\nExpert剩余「{}」首\nMaster剩余「{}」首\n",
        version,
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        target = spec.target.label(),
    );
    if counts[4] > 0 || matches!(version.as_str(), "舞" | "霸") {
        text.push_str(&format!("Re:Master剩余「{}」首\n", counts[4]));
    }
    let listed_count;
    if !difficult.is_empty() {
        listed_count = difficult.len();
        if difficult.len() < 60 {
            text.push_str("剩余定数大于13.6的曲目：\n");
            append_lines(&mut text, &difficult);
        } else {
            text.push_str(&format!(
                "还有{}首大于13.6定数的曲目，加油推分捏！\n",
                difficult.len()
            ));
        }
    } else if !remaining.is_empty() {
        listed_count = remaining.len();
        if remaining.len() < 60 {
            text.push_str("剩余曲目：\n");
            append_lines(&mut text, &remaining.iter().collect::<Vec<_>>());
        } else {
            text.push_str("已经没有定数大于13.6的曲目了，加油清谱捏！\n");
        }
    } else {
        listed_count = 0;
        text = format!(
            "已经没有剩余的的曲目了，恭喜{username}完成「{}{target}」！",
            version,
            target = spec.target.label()
        );
    }
    Ok(PreparedPlateProgress {
        version,
        target: spec.target,
        server,
        text,
        listed_count,
    })
}

fn append_member(
    member: &PlateMember,
    target: CompletionTarget,
    records: &HashMap<ChartKey, &B50Chart>,
    counts: &mut [usize; 5],
    remaining: &mut Vec<RemainingChart>,
) -> Result<(), CompletionError> {
    for chart in member.charts() {
        let Some(index) = difficulty_index(chart.difficulty()) else {
            continue;
        };
        let record = chart.key().and_then(|key| records.get(key).copied());
        let state = records::state(record, target)?;
        if state.completed {
            continue;
        }
        counts[index] += 1;
        let record_text = match target {
            CompletionTarget::Achievement(_) => state.achievement.map(achievement_text),
            CompletionTarget::FullCombo(_) => state.combo.map(|value| value.label().to_owned()),
            CompletionTarget::FullSync(_) => state.sync.map(|value| value.label().to_owned()),
            CompletionTarget::PlateGeneral => state.achievement.map(achievement_text),
            CompletionTarget::PlateExtreme | CompletionTarget::PlateGod => {
                state.combo.map(|value| value.label().to_owned())
            }
            CompletionTarget::PlateDance => state.sync.map(|value| value.label().to_owned()),
        };
        remaining.push(RemainingChart {
            id: display_id(member.identity().display_id()),
            title: member.title().to_owned(),
            difficulty: chart.difficulty(),
            constant: chart.constant(),
            record: record_text,
        });
    }
    Ok(())
}

fn achievement_text(value: maimai_core::AchievementRate) -> String {
    let scaled = value.ten_thousandths();
    let fraction = format!("{:04}", scaled % 10_000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{}.0%", scaled / 10_000)
    } else {
        format!("{}.{}%", scaled / 10_000, fraction)
    }
}

fn append_lines(text: &mut String, values: &[&RemainingChart]) {
    for (index, chart) in values.iter().enumerate() {
        let constant = chart
            .constant
            .map(|value| value.value().normalize().to_string())
            .unwrap_or_else(|| "-".to_owned());
        let constant = if constant != "-" && !constant.contains('.') {
            format!("{constant}.0")
        } else {
            constant
        };
        text.push_str(&format!(
            "No.{:02} {:>7} {:>11} 「{}」 {}  {}\n",
            index + 1,
            format!("「{}」", chart.id),
            format!("「{}」", difficulty_label(chart.difficulty)),
            constant,
            chart.title,
            chart.record.as_deref().unwrap_or("")
        ));
    }
}

fn difficulty_index(value: Difficulty) -> Option<usize> {
    match value {
        Difficulty::Basic => Some(0),
        Difficulty::Advanced => Some(1),
        Difficulty::Expert => Some(2),
        Difficulty::Master => Some(3),
        Difficulty::ReMaster => Some(4),
        Difficulty::Utage => None,
    }
}

fn difficulty_label(value: Difficulty) -> &'static str {
    match value {
        Difficulty::Basic => "Basic",
        Difficulty::Advanced => "Advanced",
        Difficulty::Expert => "Expert",
        Difficulty::Master => "Master",
        Difficulty::ReMaster => "Re:Master",
        Difficulty::Utage => "Utage",
    }
}

fn normalized_version(value: &str, server: PlateServer) -> String {
    let value = value.trim();
    if server == PlateServer::Jp
        && matches!(
            value.replace([' ', '　'], "").to_ascii_lowercase().as_str(),
            "circle" | "maimaiでらっくすcircle"
        )
    {
        "丸".to_owned()
    } else {
        value.to_owned()
    }
}
