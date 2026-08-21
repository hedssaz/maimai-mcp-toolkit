use time::{Duration, OffsetDateTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DailyResetHour(u8);

impl DailyResetHour {
    pub const UTC_14: Self = Self(14);

    pub const fn new(value: u8) -> Option<Self> {
        if value < 24 { Some(Self(value)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

pub fn latest(now: OffsetDateTime, hour: DailyResetHour) -> OffsetDateTime {
    let day = now
        - Duration::hours(i64::from(now.hour()))
        - Duration::minutes(i64::from(now.minute()))
        - Duration::seconds(i64::from(now.second()))
        - Duration::nanoseconds(i64::from(now.nanosecond()));
    let reset = day + Duration::hours(i64::from(hour.get()));
    if now < reset {
        reset - Duration::days(1)
    } else {
        reset
    }
}

pub fn next(now: OffsetDateTime, hour: DailyResetHour) -> OffsetDateTime {
    latest(now, hour) + Duration::days(1)
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime};

    use super::{DailyResetHour, latest, next};

    #[test]
    fn reset_boundary_is_stable_before_and_after_hour() {
        let day = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
        let before = day + Duration::hours(13);
        let after = day + Duration::hours(15);
        assert_eq!(
            latest(before, DailyResetHour::UTC_14),
            day - Duration::hours(10)
        );
        assert_eq!(
            latest(after, DailyResetHour::UTC_14),
            day + Duration::hours(14)
        );
        assert_eq!(
            next(after, DailyResetHour::UTC_14),
            day + Duration::hours(14) + Duration::days(1)
        );
        assert!(DailyResetHour::new(24).is_none());
    }
}
