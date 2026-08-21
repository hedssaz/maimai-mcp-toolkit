use std::{cmp::Ordering, fmt};

use num_rational::Ratio;

use super::ScoringError;

/// Exact percentage value backed by `num-rational`.
///
/// Arithmetic is checked before constructing a normalized ratio so malformed or
/// excessively large MCP input becomes a structured domain error rather than a
/// panic or wrapped integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Rational(Ratio<i128>);

impl Rational {
    pub(crate) fn new(numerator: i128, denominator: i128) -> Result<Self, ScoringError> {
        if denominator == 0 {
            return Err(ScoringError::Arithmetic("rational denominator is zero"));
        }
        let (numerator, denominator) = if denominator < 0 {
            (
                numerator
                    .checked_neg()
                    .ok_or(ScoringError::Arithmetic("rational sign overflow"))?,
                denominator
                    .checked_neg()
                    .ok_or(ScoringError::Arithmetic("rational sign overflow"))?,
            )
        } else {
            (numerator, denominator)
        };
        Ok(Self(Ratio::new(numerator, denominator)))
    }

    pub(crate) fn integer(value: i128) -> Self {
        Self(Ratio::from_integer(value))
    }

    pub(crate) fn numerator(self) -> i128 {
        *self.0.numer()
    }

    pub(crate) fn denominator(self) -> i128 {
        *self.0.denom()
    }

    pub(crate) fn add(self, other: Self) -> Result<Self, ScoringError> {
        let left = self
            .numerator()
            .checked_mul(other.denominator())
            .ok_or(ScoringError::Arithmetic("rational addition overflow"))?;
        let right = other
            .numerator()
            .checked_mul(self.denominator())
            .ok_or(ScoringError::Arithmetic("rational addition overflow"))?;
        let numerator = left
            .checked_add(right)
            .ok_or(ScoringError::Arithmetic("rational addition overflow"))?;
        let denominator = self
            .denominator()
            .checked_mul(other.denominator())
            .ok_or(ScoringError::Arithmetic("rational addition overflow"))?;
        Self::new(numerator, denominator)
    }

    pub(crate) fn subtract(self, other: Self) -> Result<Self, ScoringError> {
        let negated = other
            .numerator()
            .checked_neg()
            .ok_or(ScoringError::Arithmetic("rational subtraction overflow"))?;
        self.add(Self::new(negated, other.denominator())?)
    }

    pub(crate) fn multiply_integer(self, value: i128) -> Result<Self, ScoringError> {
        Self::new(
            self.numerator()
                .checked_mul(value)
                .ok_or(ScoringError::Arithmetic("rational multiplication overflow"))?,
            self.denominator(),
        )
    }

    pub(crate) fn divide_integer(self, value: i128) -> Result<Self, ScoringError> {
        Self::new(
            self.numerator(),
            self.denominator()
                .checked_mul(value)
                .ok_or(ScoringError::Arithmetic("rational division overflow"))?,
        )
    }

    pub(crate) fn cmp_checked(self, other: Self) -> Result<Ordering, ScoringError> {
        let left = self
            .numerator()
            .checked_mul(other.denominator())
            .ok_or(ScoringError::Arithmetic("rational comparison overflow"))?;
        let right = other
            .numerator()
            .checked_mul(self.denominator())
            .ok_or(ScoringError::Arithmetic("rational comparison overflow"))?;
        Ok(left.cmp(&right))
    }

    pub(crate) fn floor(self) -> i128 {
        self.numerator().div_euclid(self.denominator())
    }

    pub(crate) fn ceil(self) -> i128 {
        let quotient = self.floor();
        if self.numerator().rem_euclid(self.denominator()) == 0 {
            quotient
        } else {
            quotient + 1
        }
    }

    pub(crate) fn is_integer(self) -> bool {
        self.0.is_integer()
    }

    pub(crate) fn decimal_string(self, places: u32) -> Result<String, ScoringError> {
        let scale = pow10(places)?;
        let scaled_numerator = self
            .numerator()
            .checked_mul(scale)
            .ok_or(ScoringError::Arithmetic("decimal formatting overflow"))?;
        let mut rounded = scaled_numerator.div_euclid(self.denominator());
        let remainder = scaled_numerator.rem_euclid(self.denominator());
        let doubled = remainder
            .checked_mul(2)
            .ok_or(ScoringError::Arithmetic("decimal formatting overflow"))?;
        if doubled > self.denominator() || (doubled == self.denominator() && rounded % 2 != 0) {
            rounded = rounded
                .checked_add(1)
                .ok_or(ScoringError::Arithmetic("decimal formatting overflow"))?;
        }
        Ok(trim_decimal(format_scaled(rounded, places)))
    }

    pub(crate) fn display_floor(self, digits: u32) -> Result<(String, i128), ScoringError> {
        let scale = pow10(digits)?;
        let scaled = self
            .numerator()
            .checked_mul(scale)
            .ok_or(ScoringError::Arithmetic("percentage display overflow"))?
            .div_euclid(self.denominator());
        Ok((format_scaled(scaled, digits), scaled))
    }

    pub(crate) fn display_half_up(self, digits: u32) -> Result<(String, i128), ScoringError> {
        let scale = pow10(digits)?;
        let scaled_numerator = self
            .numerator()
            .checked_mul(scale)
            .ok_or(ScoringError::Arithmetic("percentage display overflow"))?;
        let mut scaled = scaled_numerator.div_euclid(self.denominator());
        let remainder = scaled_numerator.rem_euclid(self.denominator());
        if remainder
            .checked_mul(2)
            .ok_or(ScoringError::Arithmetic("percentage display overflow"))?
            >= self.denominator()
        {
            scaled = scaled
                .checked_add(1)
                .ok_or(ScoringError::Arithmetic("percentage display overflow"))?;
        }
        Ok((format_scaled(scaled, digits), scaled))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

pub(crate) fn parse_decimal(value: &str, field: &'static str) -> Result<Rational, ScoringError> {
    let trimmed = value.trim();
    let value = trimmed.strip_suffix('%').unwrap_or(trimmed).trim();
    if value.is_empty() {
        return Err(ScoringError::InvalidDecimal { field });
    }

    let (negative, unsigned) = match value.as_bytes().first().copied() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let mut exponent_parts = unsigned.split(['e', 'E']);
    let mantissa = exponent_parts.next().unwrap_or_default();
    let exponent_text = exponent_parts.next();
    if exponent_parts.next().is_some() {
        return Err(ScoringError::InvalidDecimal { field });
    }
    let exponent = match exponent_text {
        Some(text) if !text.is_empty() => text
            .parse::<i32>()
            .map_err(|_| ScoringError::InvalidDecimal { field })?,
        Some(_) => return Err(ScoringError::InvalidDecimal { field }),
        None => 0,
    };

    let mut pieces = mantissa.split('.');
    let integer = pieces.next().unwrap_or_default();
    let fraction = pieces.next();
    let has_any_digits = !integer.is_empty() || fraction.is_some_and(|part| !part.is_empty());
    if pieces.next().is_some()
        || !has_any_digits
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(ScoringError::InvalidDecimal { field });
    }

    let integer_value = if integer.is_empty() {
        0
    } else {
        integer
            .parse::<i128>()
            .map_err(|_| ScoringError::InvalidDecimal { field })?
    };
    let fraction_text = fraction.unwrap_or_default();
    let fraction_digits =
        i64::try_from(fraction_text.len()).map_err(|_| ScoringError::InvalidDecimal { field })?;
    let fraction_value = if fraction_text.is_empty() {
        0
    } else {
        fraction_text
            .parse::<i128>()
            .map_err(|_| ScoringError::InvalidDecimal { field })?
    };
    let mantissa_scale =
        pow10(u32::try_from(fraction_digits).map_err(|_| ScoringError::InvalidDecimal { field })?)?;
    let unsigned_mantissa = integer_value
        .checked_mul(mantissa_scale)
        .and_then(|value| value.checked_add(fraction_value))
        .ok_or(ScoringError::Arithmetic("decimal parsing overflow"))?;
    let decimal_scale = fraction_digits
        .checked_sub(i64::from(exponent))
        .ok_or(ScoringError::Arithmetic("decimal exponent overflow"))?;
    let (unsigned_numerator, denominator) = if decimal_scale >= 0 {
        let power =
            u32::try_from(decimal_scale).map_err(|_| ScoringError::InvalidDecimal { field })?;
        (unsigned_mantissa, pow10(power)?)
    } else {
        let power =
            u32::try_from(-decimal_scale).map_err(|_| ScoringError::InvalidDecimal { field })?;
        (
            unsigned_mantissa
                .checked_mul(pow10(power)?)
                .ok_or(ScoringError::Arithmetic("decimal parsing overflow"))?,
            1,
        )
    };
    let numerator = if negative {
        unsigned_numerator
            .checked_neg()
            .ok_or(ScoringError::Arithmetic("decimal parsing overflow"))?
    } else {
        unsigned_numerator
    };
    Rational::new(numerator, denominator)
}

pub(crate) fn pow10(power: u32) -> Result<i128, ScoringError> {
    10_i128
        .checked_pow(power)
        .ok_or(ScoringError::Arithmetic("decimal scale overflow"))
}

fn format_scaled(value: i128, digits: u32) -> String {
    if digits == 0 {
        return value.to_string();
    }
    let negative = value < 0;
    let absolute = value.unsigned_abs();
    let scale = 10_u128.pow(digits);
    let integer = absolute / scale;
    let fraction = absolute % scale;
    let sign = if negative { "-" } else { "" };
    format!(
        "{sign}{integer}.{fraction:0width$}",
        width = digits as usize
    )
}

fn trim_decimal(mut value: String) -> String {
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    if value == "-0" { "0".to_owned() } else { value }
}

#[cfg(test)]
mod tests {
    use super::parse_decimal;
    use crate::scoring::ScoringError;

    #[test]
    fn decimal_parser_and_display_are_exact() -> Result<(), ScoringError> {
        let value = parse_decimal("100.49995%", "target")?;
        assert_eq!(value.to_string(), "2009999/20000");
        assert_eq!(value.display_floor(4)?, ("100.4999".to_owned(), 1_004_999));
        assert_eq!(
            value.display_half_up(4)?,
            ("100.5000".to_owned(), 1_005_000)
        );
        assert_eq!(parse_decimal(".5", "target")?.to_string(), "1/2");
        assert_eq!(parse_decimal("1.", "target")?.to_string(), "1");
        assert_eq!(
            parse_decimal("1.004999e2", "target")?.to_string(),
            "1004999/10000"
        );
        Ok(())
    }
}
