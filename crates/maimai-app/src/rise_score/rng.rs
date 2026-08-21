pub(super) trait RiseRandom {
    fn below(&mut self, upper: usize) -> usize;

    fn weighted(&mut self, weights: &[u64]) -> usize {
        let total = weights.iter().copied().sum::<u64>();
        if total == 0 {
            return 0;
        }
        let mut value = self.next_u64() % total;
        for (index, weight) in weights.iter().copied().enumerate() {
            if value < weight {
                return index;
            }
            value -= weight;
        }
        weights.len().saturating_sub(1)
    }

    fn next_u64(&mut self) -> u64;
}

pub(super) struct XorShift64(u64);

impl XorShift64 {
    pub(super) fn seeded(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }
}

impl RiseRandom for XorShift64 {
    fn below(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            return 0;
        }
        let upper = u64::try_from(upper).unwrap_or(u64::MAX);
        usize::try_from(self.next_u64() % upper).unwrap_or(0)
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
}

#[cfg(test)]
pub(super) struct FixedRandom {
    values: std::collections::VecDeque<u64>,
}

#[cfg(test)]
impl FixedRandom {
    pub(super) fn new(values: impl IntoIterator<Item = u64>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl RiseRandom for FixedRandom {
    fn below(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            return 0;
        }
        usize::try_from(self.next_u64() % u64::try_from(upper).unwrap_or(u64::MAX)).unwrap_or(0)
    }

    fn next_u64(&mut self) -> u64 {
        self.values.pop_front().unwrap_or(0)
    }
}
