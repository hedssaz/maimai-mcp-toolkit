use std::{error::Error, fmt};

/// “今日舞萌”依次判定的固定活动。顺序会影响位运算结果，属于兼容接口。
pub const ACTIVITIES: [&str; 11] = [
    "拼机",
    "推分",
    "越级",
    "下埋",
    "夜勤",
    "练底力",
    "练手法",
    "打旧框",
    "干饭",
    "抓绝赞",
    "收歌",
];

/// main 分支当前使用的固定提醒文案。
pub const REMINDER: &str = "杨树森：在当前的知识水平下，以内屏为主的，可能有人品问题";

/// 调用方先按所需时区取得月、日，再把这个纯值传入核心规则。
///
/// 结构中刻意没有年份：旧 `qqhash` 只使用月和日。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TodayDate {
    month: u8,
    day: u8,
}

impl TodayDate {
    pub fn new(month: u8, day: u8) -> Result<Self, TodayError> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(TodayError::InvalidDate { month, day });
        }
        Ok(Self { month, day })
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }
}

/// 可由搜索服务组装后注入的候选曲目；核心规则不读取曲库或全局状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodaySong {
    pub id: String,
    pub title: String,
    pub chart_constants: Vec<String>,
}

impl TodaySong {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        chart_constants: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            chart_constants: chart_constants.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodayResult<'a, T> {
    pub rp: u8,
    pub good: Vec<&'static str>,
    pub bad: Vec<&'static str>,
    pub song: &'a T,
    pub offset: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TodayError {
    InvalidDate { month: u8, day: u8 },
    EmptySongs,
    ArithmeticOverflow,
}

impl fmt::Display for TodayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDate { month, day } => {
                write!(formatter, "无效的月日：{month:02}-{day:02}")
            }
            Self::EmptySongs => formatter.write_str("songs 不能为空"),
            Self::ArithmeticOverflow => formatter.write_str("今日舞萌哈希计算溢出"),
        }
    }
}

impl Error for TodayError {}

/// 精确保留旧公式：`((day + 31 * month + 77 + offset) * qq) >> 8`。
///
/// 使用 `i128` 保留负 offset 的算术右移语义，同时避免常规 QQ 值在乘法中溢出。
pub fn qqhash(qq: u64, date: TodayDate, offset: i64) -> Result<i128, TodayError> {
    let days = i128::from(date.day())
        .checked_add(i128::from(date.month()) * 31)
        .and_then(|value| value.checked_add(77))
        .and_then(|value| value.checked_add(i128::from(offset)))
        .ok_or(TodayError::ArithmeticOverflow)?;
    let product = days
        .checked_mul(i128::from(qq))
        .ok_or(TodayError::ArithmeticOverflow)?;
    Ok(product >> 8)
}

/// Rust 风格别名；接线时可保留旧接口名 `qqhash`。
pub fn qq_hash(qq: u64, date: TodayDate, offset: i64) -> Result<i128, TodayError> {
    qqhash(qq, date, offset)
}

/// 计算宜忌后继续使用已经右移 22 位的哈希选歌。
pub fn today_maimai<'a, T>(
    qq: u64,
    songs: &'a [T],
    date: TodayDate,
    offset: i64,
) -> Result<TodayResult<'a, T>, TodayError> {
    if songs.is_empty() {
        return Err(TodayError::EmptySongs);
    }

    let mut shifted_hash = qqhash(qq, date, offset)?;
    let rp = shifted_hash.rem_euclid(100) as u8;
    let mut good = Vec::new();
    let mut bad = Vec::new();

    for activity in ACTIVITIES {
        match shifted_hash & 3 {
            3 => good.push(activity),
            0 => bad.push(activity),
            _ => {}
        }
        shifted_hash >>= 2;
    }

    let song_count = i128::try_from(songs.len()).map_err(|_| TodayError::ArithmeticOverflow)?;
    let song_index = usize::try_from(shifted_hash.rem_euclid(song_count))
        .map_err(|_| TodayError::ArithmeticOverflow)?;
    let song = songs
        .get(song_index)
        .ok_or(TodayError::ArithmeticOverflow)?;

    Ok(TodayResult {
        rp,
        good,
        bad,
        song,
        offset,
    })
}

pub fn format_today_maimai(
    bot_name: &str,
    qq: u64,
    songs: &[TodaySong],
    date: TodayDate,
    offset: i64,
) -> Result<String, TodayError> {
    let result = today_maimai(qq, songs, date, offset)?;
    let mut lines = vec![format!("今日人品值：{}", result.rp)];

    lines.extend(result.good.iter().map(|activity| format!("宜 {activity}")));
    lines.extend(result.bad.iter().map(|activity| format!("忌 {activity}")));
    lines.push(format!("{bot_name}提醒您：{REMINDER}"));
    lines.push("今日推荐歌曲：".to_owned());
    lines.push(format!("ID.{} - {}", result.song.id, result.song.title));
    lines.push(result.song.chart_constants.join("/"));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{TodayDate, TodayError, TodaySong, format_today_maimai, qqhash, today_maimai};

    fn date() -> Result<TodayDate, TodayError> {
        TodayDate::new(5, 31)
    }

    fn songs() -> Vec<TodaySong> {
        vec![
            TodaySong::new("1", "A", ["1.0"]),
            TodaySong::new("2", "B", ["2.0"]),
            TodaySong::new("3", "C", ["3.0"]),
        ]
    }

    #[test]
    fn keeps_shifted_hash_activity_and_song_semantics() -> Result<(), TodayError> {
        let date = date()?;
        let songs = songs();
        let result = today_maimai(123_456_789, &songs, date, 0)?;

        assert_eq!(qqhash(123_456_789, date, 0)?, 126_832_560);
        assert_eq!(result.rp, 60);
        assert_eq!(result.good, ["越级", "夜勤", "练底力", "干饭", "抓绝赞"]);
        assert_eq!(result.bad, ["拼机", "推分", "练手法", "收歌"]);
        assert_eq!(result.song.title, "A");
        Ok(())
    }

    #[test]
    fn offset_changes_original_hash_input() -> Result<(), TodayError> {
        let date = date()?;
        let songs = songs();
        let result = today_maimai(123_456_789, &songs, date, 7)?;

        assert_eq!(qqhash(123_456_789, date, 7)?, 130_208_332);
        assert_eq!(result.rp, 32);
        assert_eq!(result.song.title, "B");
        Ok(())
    }

    #[test]
    fn formats_the_current_main_message_exactly() -> Result<(), TodayError> {
        let songs = [TodaySong::new(
            "834",
            "PANDORA PARADOXXX",
            ["6.0", "8.0", "13.4", "14.8"],
        )];
        let text = format_today_maimai("铃", 123_456_789, &songs, date()?, 0)?;

        assert_eq!(
            text,
            concat!(
                "今日人品值：60\n",
                "宜 越级\n",
                "宜 夜勤\n",
                "宜 练底力\n",
                "宜 干饭\n",
                "宜 抓绝赞\n",
                "忌 拼机\n",
                "忌 推分\n",
                "忌 练手法\n",
                "忌 收歌\n",
                "铃提醒您：杨树森：在当前的知识水平下，以内屏为主的，可能有人品问题\n",
                "今日推荐歌曲：\n",
                "ID.834 - PANDORA PARADOXXX\n",
                "6.0/8.0/13.4/14.8"
            )
        );
        Ok(())
    }

    #[test]
    fn rejects_an_empty_candidate_list() -> Result<(), TodayError> {
        let error = today_maimai::<TodaySong>(123, &[], date()?, 0);
        assert_eq!(error, Err(TodayError::EmptySongs));
        Ok(())
    }
}
