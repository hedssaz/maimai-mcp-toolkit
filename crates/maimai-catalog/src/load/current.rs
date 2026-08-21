use crate::raw::DivingFishSong;

pub(super) fn current_diving_fish_versions(
    songs: &[DivingFishSong],
    version_order: &[String],
) -> Vec<String> {
    let ranked = version_order
        .iter()
        .enumerate()
        .map(|(index, version)| (normalize(version), index))
        .collect::<Vec<_>>();
    let mut known = Vec::new();
    let mut unknown_new = Vec::new();
    for song in songs {
        let version = song.basic_info.version.trim();
        if version.is_empty() {
            continue;
        }
        let normalized = normalize(version);
        let rank = ranked
            .iter()
            .filter(|(candidate, _)| !candidate.is_empty() && normalized.ends_with(candidate))
            .map(|(_, index)| *index)
            .max();
        match rank {
            Some(rank) => known.push((version.to_owned(), rank)),
            None if song.basic_info.is_new => unknown_new.push(version.to_owned()),
            None => {}
        }
    }
    let mut output = if unknown_new.is_empty() {
        known
            .iter()
            .map(|(_, rank)| *rank)
            .max()
            .map_or_else(Vec::new, |latest| {
                known
                    .into_iter()
                    .filter_map(|(version, rank)| (rank == latest).then_some(version))
                    .collect()
            })
    } else {
        unknown_new
    };
    if output.is_empty() {
        output.extend(
            songs
                .iter()
                .filter(|song| song.basic_info.is_new)
                .map(|song| song.basic_info.version.trim())
                .filter(|version| !version.is_empty())
                .map(str::to_owned),
        );
    }
    output.sort();
    output.dedup();
    output
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
