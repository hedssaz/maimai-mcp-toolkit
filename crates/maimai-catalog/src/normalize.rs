use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, Default)]
pub struct TextNormalizer {
    simplified_to_traditional: HashMap<char, char>,
}

impl TextNormalizer {
    pub fn new(simplified_to_traditional: HashMap<char, char>) -> Self {
        Self {
            simplified_to_traditional,
        }
    }

    pub fn normalize(&self, value: &str) -> String {
        value
            .nfkc()
            .flat_map(char::to_lowercase)
            .map(|character| {
                self.simplified_to_traditional
                    .get(&character)
                    .copied()
                    .unwrap_or(character)
            })
            .collect::<String>()
            .trim()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::TextNormalizer;

    #[test]
    fn normalizes_width_case_and_simplified_characters() {
        let normalizer = TextNormalizer::new(HashMap::from([('爱', '愛')]));
        assert_eq!(normalizer.normalize(" ＬＯＶＥ爱 "), "love愛");
    }
}
