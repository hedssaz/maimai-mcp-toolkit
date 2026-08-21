use serde::Deserialize;
use serde_json::Number;

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum Scalar {
    Text(String),
    Number(Number),
}

impl Scalar {
    pub(super) fn text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Number(value) => value.to_string(),
        }
    }

    pub(super) fn level_text(&self) -> String {
        let Self::Number(number) = self else {
            return self.text();
        };
        let value = number.to_string();
        let Some((integer, fraction)) = value.split_once('.') else {
            return value;
        };
        if !integer.is_empty()
            && fraction.bytes().all(|value| value == b'0')
            && integer.bytes().all(|value| value.is_ascii_digit())
        {
            integer.to_owned()
        } else {
            value
        }
    }
}

#[derive(Default, Deserialize)]
pub(super) struct IdentityDto {
    pub(super) qq: Option<Scalar>,
    pub(super) username: Option<Scalar>,
    #[serde(rename = "scoreSource")]
    pub(super) score_source_camel: Option<String>,
    pub(super) score_source: Option<String>,
    #[serde(rename = "dataSource")]
    pub(super) data_source_camel: Option<String>,
    pub(super) data_source: Option<String>,
    pub(super) source: Option<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct PlateArgs {
    #[serde(flatten)]
    pub(super) identity: IdentityDto,
    pub(super) version: Option<Scalar>,
    pub(super) plate: Option<Scalar>,
    pub(super) plan: Option<String>,
    pub(super) server: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum PlateItemDto {
    Object(Box<PlateArgs>),
    Version(Scalar),
    Invalid(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum ScalarList {
    One(Scalar),
    Many(Vec<Scalar>),
}

impl ScalarList {
    pub(super) fn into_vec(self) -> Vec<Scalar> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum TextList {
    One(String),
    Many(Vec<String>),
}

impl TextList {
    pub(super) fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Default, Deserialize)]
pub(super) struct PlateBatchArgs {
    #[serde(flatten)]
    pub(super) identity: IdentityDto,
    #[serde(default)]
    pub(super) items: Vec<PlateItemDto>,
    pub(super) versions: Option<ScalarList>,
    pub(super) version: Option<Scalar>,
    pub(super) plans: Option<TextList>,
    pub(super) plan: Option<String>,
    pub(super) server: Option<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct ProgressArgs {
    #[serde(flatten)]
    pub(super) identity: IdentityDto,
    pub(super) level: Option<Scalar>,
    pub(super) plan: Option<String>,
    pub(super) server: Option<String>,
    pub(super) category: Option<String>,
    pub(super) page: Option<i64>,
}

#[derive(Default, Deserialize)]
pub(super) struct RatingArgs {
    #[serde(flatten)]
    pub(super) identity: IdentityDto,
    pub(super) rating: Option<Scalar>,
    #[serde(default)]
    pub(super) isfc: bool,
}
