pub(crate) const fn star_level(score: u64, maximum: u64) -> Option<u8> {
    if maximum == 0 {
        return None;
    }
    let score = score.saturating_mul(100);
    let level = if score <= maximum.saturating_mul(85) {
        0
    } else if score <= maximum.saturating_mul(90) {
        1
    } else if score <= maximum.saturating_mul(93) {
        2
    } else if score <= maximum.saturating_mul(95) {
        3
    } else if score <= maximum.saturating_mul(97) {
        4
    } else {
        5
    };
    Some(level)
}

#[cfg(test)]
mod tests {
    use super::star_level;

    #[test]
    fn boundaries_use_exact_cross_multiplication() {
        assert_eq!(star_level(85_000, 100_000), Some(0));
        assert_eq!(star_level(85_001, 100_000), Some(1));
        assert_eq!(star_level(90_000, 100_000), Some(1));
        assert_eq!(star_level(90_001, 100_000), Some(2));
        assert_eq!(star_level(93_000, 100_000), Some(2));
        assert_eq!(star_level(95_000, 100_000), Some(3));
        assert_eq!(star_level(97_000, 100_000), Some(4));
        assert_eq!(star_level(97_001, 100_000), Some(5));
        assert_eq!(star_level(1, 0), None);
    }
}
