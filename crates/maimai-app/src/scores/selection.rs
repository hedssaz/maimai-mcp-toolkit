use std::collections::HashMap;

use maimai_core::ChartKey;

use super::B50Chart;

pub(crate) fn best_by_chart(records: &[B50Chart]) -> HashMap<ChartKey, &B50Chart> {
    let mut best = HashMap::new();
    for record in records {
        best.entry(record.key.clone())
            .and_modify(|current: &mut &B50Chart| {
                if score_order(record) > score_order(current) {
                    *current = record;
                }
            })
            .or_insert(record);
    }
    best
}

fn score_order(record: &B50Chart) -> (Option<u32>, Option<u32>, Option<u32>) {
    (
        record.achievements.map(|value| value.ten_thousandths()),
        record.dx_score,
        record.rating,
    )
}

#[cfg(test)]
mod tests {
    use maimai_core::{
        AchievementRate, ChartGeneration, ChartKey, Difficulty, PlayAchievement, SongIdNamespace,
        SourceSongId, UtageScore,
    };

    use super::best_by_chart;
    use crate::scores::B50Chart;

    #[test]
    fn chooses_achievement_then_dx_score_then_rating_and_keeps_first_tie()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::Lxns, 383),
            ChartGeneration::Deluxe,
            Difficulty::Master,
        )?;
        let first = chart(key.clone(), "first", "100", 900, 300)?;
        let lower_achievement = chart(key.clone(), "lower", "99.9999", 9_999, 9_999)?;
        let better_dx = chart(key.clone(), "better dx", "100", 901, 1)?;
        let better_rating = chart(key.clone(), "better rating", "100", 901, 301)?;
        let tied = chart(key.clone(), "tied later", "100", 901, 301)?;
        let records = vec![first, lower_achievement, better_dx, better_rating, tied];

        let best = best_by_chart(&records);
        assert_eq!(
            best.get(&key).map(|record| record.title.as_str()),
            Some("better rating")
        );
        Ok(())
    }

    #[test]
    fn utage_duplicates_compare_full_units_instead_of_missing_ranked_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = ChartKey::new(
            SourceSongId::numeric(SongIdNamespace::Lxns, 111_597),
            ChartGeneration::UtageOnePlayer,
            Difficulty::Utage,
        )?;
        let mut lower = chart(key.clone(), "lower", "100", 9_999, 9_999)?;
        lower.achievements = Some(PlayAchievement::from(UtageScore::from_ten_thousandths(
            1_500_000,
        )));
        let mut higher = lower.clone();
        higher.title = "higher".to_owned();
        higher.achievements = Some(PlayAchievement::from(UtageScore::from_ten_thousandths(
            1_535_756,
        )));

        let records = vec![lower, higher];
        let best = best_by_chart(&records);
        assert_eq!(
            best.get(&key).map(|record| record.title.as_str()),
            Some("higher")
        );
        Ok(())
    }

    fn chart(
        key: ChartKey,
        title: &str,
        achievements: &str,
        dx_score: u32,
        rating: u32,
    ) -> Result<B50Chart, Box<dyn std::error::Error>> {
        Ok(B50Chart {
            source_song_id: key.song().clone(),
            key,
            title: title.to_owned(),
            level: "13".to_owned(),
            constant: None,
            achievements: Some(AchievementRate::from_decimal_str(achievements)?.into()),
            dx_score: Some(dx_score),
            rating: Some(rating),
            original_rating: None,
            grade: None,
            full_combo: None,
            full_sync: None,
            version: String::new(),
            is_current: false,
            fit_constant: None,
            fit_label: None,
        })
    }
}
