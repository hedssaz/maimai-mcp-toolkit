use std::{cmp::Ordering, fmt};

use num_bigint::{BigInt, BigUint, Sign};
use num_traits::{One, Zero};

/// 计数向量的精确数量。
///
/// 这是领域边界上的薄类型：任意精度算术由 `num-bigint` 负责，避免把搜索
/// 结果的序列化/API 与具体第三方类型绑定。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExactCount(BigUint);

impl ExactCount {
    pub fn zero() -> Self {
        Self(BigUint::zero())
    }

    pub fn one() -> Self {
        Self(BigUint::one())
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn add_assign(&mut self, other: &Self) {
        self.0 += &other.0;
    }

    pub fn multiplied(&self, other: &Self) -> Self {
        Self(&self.0 * &other.0)
    }
}

impl From<u64> for ExactCount {
    fn from(value: u64) -> Self {
        Self(BigUint::from(value))
    }
}

impl Ord for ExactCount {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for ExactCount {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ExactCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// `x_1 + ... + x_n = total` 且 `0 <= x_i <= caps_i` 的精确解数。
pub(crate) fn bounded_composition_count(total: u32, caps: &[u32]) -> ExactCount {
    if caps.is_empty() {
        return ExactCount::from(u64::from(total == 0));
    }

    // 一个 note type 只有八种判定，子集枚举上限固定为 256。
    let subset_count = 1_usize << caps.len();
    let mut result = BigInt::zero();
    for mask in 0..subset_count {
        let mut shifted = i128::from(total);
        let mut selected = 0_u32;
        for (index, cap) in caps.iter().copied().enumerate() {
            if mask & (1_usize << index) != 0 {
                shifted -= i128::from(cap) + 1;
                selected += 1;
            }
        }
        if shifted < 0 {
            continue;
        }
        let n = shifted as u64 + caps.len() as u64 - 1;
        let k = caps.len() as u64 - 1;
        let ways = BigInt::from_biguint(Sign::Plus, binomial(n, k).0);
        if selected.is_multiple_of(2) {
            result += ways;
        } else {
            result -= ways;
        }
    }
    match result.to_biguint() {
        Some(value) => ExactCount(value),
        None => {
            debug_assert!(false, "bounded composition count became negative");
            ExactCount::zero()
        }
    }
}

fn binomial(n: u64, k: u64) -> ExactCount {
    let k = k.min(n.saturating_sub(k));
    let mut value = BigUint::one();
    for divisor in 1..=k {
        value *= n - k + divisor;
        value /= divisor;
    }
    ExactCount(value)
}

#[cfg(test)]
mod tests {
    use super::{ExactCount, bounded_composition_count};

    #[test]
    fn multiplication_keeps_values_beyond_u128() {
        let factor = ExactCount::from(10_000_000_000_000_000_000_u64);
        let mut value = factor.clone();
        for _ in 0..5 {
            value = value.multiplied(&factor);
        }
        assert_eq!(value.to_string(), format!("1{}", "0".repeat(114)));
    }

    #[test]
    fn bounded_count_matches_small_enumeration() {
        assert_eq!(
            bounded_composition_count(3, &[3, 3, 3]),
            ExactCount::from(10)
        );
        assert_eq!(
            bounded_composition_count(3, &[1, 1, 1]),
            ExactCount::from(1)
        );
        assert_eq!(bounded_composition_count(4, &[1, 1, 1]), ExactCount::zero());
    }
}
