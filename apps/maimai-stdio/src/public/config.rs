use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use maimai_providers::{LxnsOAuthClient, NapCatConfig, napcat::DEFAULT_NAPCAT_BASE_URL};
use secrecy::SecretString;
use thiserror::Error;
use time::UtcOffset;
use url::{ParseError, Url};

use crate::{
    oauth::{LxnsConfigError, public_oauth_client_from_env},
    runtime_paths::{RuntimePathError, RuntimePaths, configured_path},
};

const NAPCAT_BASE_URL_ENV: &str = "NAPCAT_BASE_URL";
const NAPCAT_TOKEN_ENVS: [&str; 2] = ["NAPCAT_ACCESS_TOKEN", "NAPCAT_TOKEN"];
const NAPCAT_TIMEOUT_MS_ENV: &str = "NAPCAT_TIMEOUT_MS";
const DIVING_FISH_API_BASE_URL_ENV: &str = "DIVING_FISH_API_BASE_URL";
const DIVING_FISH_COVER_BASE_URL_ENV: &str = "DIVING_FISH_COVER_BASE_URL";
const DEFAULT_DIVING_FISH_API_BASE_URL: &str = "https://www.diving-fish.com/api/";
const DEFAULT_DIVING_FISH_COVER_BASE_URL: &str = "https://www.diving-fish.com/covers/";
const STATIC_DIR_ENVS: [&str; 3] = [
    "MAIMAIDX_STATIC_DIR",
    "B50_IMAGE_YUZU_STATIC_DIR",
    "B50_IMAGE_STATIC_DIR",
];
const COVER_DIR_ENVS: [&str; 2] = ["MAIMAIDX_COVER_CACHE_DIR", "B50_IMAGE_COVER_CACHE_DIR"];
const OUTPUT_DIR_ENVS: [&str; 2] = ["MAIMAIDX_RENDER_OUTPUT_DIR", "B50_IMAGE_OUTPUT_DIR"];
const STYLE_CONFIG_ENV: &str = "B50_IMAGE_STYLE_CONFIG";
const DISPLAY_OFFSET_ENVS: [&str; 4] = [
    "MAIMAI_DISPLAY_UTC_OFFSET",
    "MCP_DISPLAY_TZ",
    "QQ_IDENTITY_DISPLAY_TZ",
    "GROUP_B50_DISPLAY_TZ",
];
pub(crate) struct RuntimeConfig {
    pub paths: RuntimePaths,
    pub napcat: NapCatConfig,
    pub oauth_client: Option<LxnsOAuthClient>,
    pub diving_fish_api_base_url: String,
    pub diving_fish_cover_base_url: String,
    pub static_root: PathBuf,
    pub cover_cache_root: PathBuf,
    pub output_root: PathBuf,
    pub style_config: PathBuf,
    pub display_offset: UtcOffset,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, PublicConfigError> {
        let paths = RuntimePaths::from_env()?;
        let cwd = env::current_dir().map_err(RuntimePathError::CurrentDirectory)?;
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let get = |name: &str| env::var_os(name);
        let static_root = existing_directory(
            resolved_path(&cwd, home.as_deref(), &get, &STATIC_DIR_ENVS)
                .unwrap_or_else(|| cwd.join("maimaidx_render_mcp/static")),
            "render static root",
        )?;
        let cover_cache_root = resolved_path(&cwd, home.as_deref(), &get, &COVER_DIR_ENVS)
            .unwrap_or_else(|| cwd.join("cover_cache"));
        let output_root = resolved_path(&cwd, home.as_deref(), &get, &OUTPUT_DIR_ENVS)
            .unwrap_or_else(|| cwd.join("maimai-images"));
        let style_config = resolved_path(&cwd, home.as_deref(), &get, &[STYLE_CONFIG_ENV])
            .unwrap_or_else(|| paths.data_dir().join("b50-image-style.json"));
        Ok(Self {
            paths,
            napcat: napcat_config(&get)?,
            oauth_client: public_oauth_client_from_env()?,
            diving_fish_api_base_url: provider_base_url(
                &get,
                DIVING_FISH_API_BASE_URL_ENV,
                DEFAULT_DIVING_FISH_API_BASE_URL,
            )?,
            diving_fish_cover_base_url: provider_base_url(
                &get,
                DIVING_FISH_COVER_BASE_URL_ENV,
                DEFAULT_DIVING_FISH_COVER_BASE_URL,
            )?,
            static_root,
            cover_cache_root,
            output_root,
            style_config,
            display_offset: display_offset(&get)?,
        })
    }
}

pub(super) type PublicConfig = RuntimeConfig;

fn napcat_config(
    get: &impl Fn(&str) -> Option<OsString>,
) -> Result<NapCatConfig, PublicConfigError> {
    let base = endpoint_text(get, NAPCAT_BASE_URL_ENV, DEFAULT_NAPCAT_BASE_URL)?;
    let timeout_ms = optional_text(get(NAPCAT_TIMEOUT_MS_ENV), NAPCAT_TIMEOUT_MS_ENV)?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| PublicConfigError::InvalidInteger {
                    name: NAPCAT_TIMEOUT_MS_ENV,
                })
        })
        .transpose()?
        .unwrap_or(10_000);
    let token = first_text(get, &NAPCAT_TOKEN_ENVS)?.map(SecretString::from);
    Ok(NapCatConfig::new(
        Url::parse(&base).map_err(|source| PublicConfigError::InvalidUrl {
            name: NAPCAT_BASE_URL_ENV,
            source,
        })?,
        Duration::from_millis(timeout_ms),
        token,
    )?)
}

fn display_offset(get: &impl Fn(&str) -> Option<OsString>) -> Result<UtcOffset, PublicConfigError> {
    let Some(value) = first_text(get, &DISPLAY_OFFSET_ENVS)? else {
        return UtcOffset::from_hms(8, 0, 0).map_err(PublicConfigError::DisplayOffset);
    };
    match value.as_str() {
        "Asia/Shanghai" | "Asia/Chongqing" | "Asia/Hong_Kong" => {
            UtcOffset::from_hms(8, 0, 0).map_err(PublicConfigError::DisplayOffset)
        }
        "UTC" | "Etc/UTC" | "Z" => Ok(UtcOffset::UTC),
        _ => parse_numeric_offset(&value),
    }
}

fn parse_numeric_offset(value: &str) -> Result<UtcOffset, PublicConfigError> {
    let (sign, body) = match value.as_bytes().first() {
        Some(b'+') => (1_i8, &value[1..]),
        Some(b'-') => (-1_i8, &value[1..]),
        _ => return Err(PublicConfigError::InvalidDisplayOffset),
    };
    let mut parts = body.split(':');
    let hours = parts.next().and_then(|part| part.parse::<i8>().ok());
    let minutes = parts.next().unwrap_or("0").parse::<i8>().ok();
    if parts.next().is_some() || hours.is_none() || minutes.is_none() {
        return Err(PublicConfigError::InvalidDisplayOffset);
    }
    UtcOffset::from_hms(
        sign.saturating_mul(hours.unwrap_or_default()),
        sign.saturating_mul(minutes.unwrap_or_default()),
        0,
    )
    .map_err(PublicConfigError::DisplayOffset)
}

fn endpoint_text(
    get: &impl Fn(&str) -> Option<OsString>,
    name: &'static str,
    fallback: &'static str,
) -> Result<String, PublicConfigError> {
    optional_text(get(name), name).map(|value| value.unwrap_or_else(|| fallback.to_owned()))
}

fn provider_base_url(
    get: &impl Fn(&str) -> Option<OsString>,
    name: &'static str,
    fallback: &'static str,
) -> Result<String, PublicConfigError> {
    let value = endpoint_text(get, name, fallback)?;
    let mut url =
        Url::parse(&value).map_err(|source| PublicConfigError::InvalidUrl { name, source })?;
    let loopback_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
    if !(url.scheme() == "https" || loopback_http)
        || url.host_str().is_none()
        || url.cannot_be_a_base()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(PublicConfigError::InvalidProviderEndpoint { name });
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url.into())
}

fn first_text(
    get: &impl Fn(&str) -> Option<OsString>,
    names: &[&'static str],
) -> Result<Option<String>, PublicConfigError> {
    for &name in names {
        if let Some(value) = optional_text(get(name), name)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn optional_text(
    value: Option<OsString>,
    name: &'static str,
) -> Result<Option<String>, PublicConfigError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| PublicConfigError::NonUnicodeEnvironment { name })?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        return Err(PublicConfigError::InvalidEnvironment { name });
    }
    Ok(Some(value))
}

fn resolved_path(
    cwd: &Path,
    home: Option<&Path>,
    get: &impl Fn(&str) -> Option<OsString>,
    names: &[&str],
) -> Option<PathBuf> {
    names.iter().find_map(|name| {
        get(name)
            .filter(|value| !value.is_empty())
            .map(|value| configured_path(cwd, Path::new(&value), home))
    })
}

fn existing_directory(path: PathBuf, label: &'static str) -> Result<PathBuf, PublicConfigError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| PublicConfigError::Directory {
            label,
            path,
            source,
        })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(PublicConfigError::NotDirectory {
            label,
            path: canonical,
        })
    }
}

#[derive(Debug, Error)]
pub enum PublicConfigError {
    #[error(transparent)]
    Paths(#[from] RuntimePathError),
    #[error(transparent)]
    OAuth(#[from] LxnsConfigError),
    #[error(transparent)]
    NapCat(#[from] maimai_providers::NapCatError),
    #[error("环境变量 {name} 不是有效 Unicode 文本")]
    NonUnicodeEnvironment { name: &'static str },
    #[error("环境变量 {name} 含控制字符")]
    InvalidEnvironment { name: &'static str },
    #[error("环境变量 {name} 不是有效整数")]
    InvalidInteger { name: &'static str },
    #[error("环境变量 {name} 不是有效 URL：{source}")]
    InvalidUrl {
        name: &'static str,
        source: ParseError,
    },
    #[error(
        "环境变量 {name} 必须是无凭据、query、fragment 的 HTTPS base URL；测试仅允许 HTTP loopback"
    )]
    InvalidProviderEndpoint { name: &'static str },
    #[error("显示时区必须是 UTC、Asia/Shanghai 或 +08:00 形式的固定偏移")]
    InvalidDisplayOffset,
    #[error("显示时区偏移无效：{0}")]
    DisplayOffset(time::error::ComponentRange),
    #[error("{label} 路径 {path} 不可用：{source}")]
    Directory {
        label: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{label} 路径不是目录：{path}")]
    NotDirectory { label: &'static str, path: PathBuf },
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, error::Error};

    use super::*;

    #[test]
    fn provider_urls_require_https_except_loopback_http() -> Result<(), Box<dyn Error>> {
        let loopback = HashMap::from([(
            DIVING_FISH_API_BASE_URL_ENV,
            OsString::from("http://127.0.0.1:3210/api"),
        )]);
        assert_eq!(
            provider_base_url(
                &|name| loopback.get(name).cloned(),
                DIVING_FISH_API_BASE_URL_ENV,
                DEFAULT_DIVING_FISH_API_BASE_URL,
            )?,
            "http://127.0.0.1:3210/api/"
        );

        for value in [
            "http://example.test/api/",
            "https://example.test/api/?token=secret",
            "https://user:secret@example.test/api/",
            "file:///tmp/provider",
        ] {
            let values = HashMap::from([(DIVING_FISH_API_BASE_URL_ENV, OsString::from(value))]);
            assert!(
                provider_base_url(
                    &|name| values.get(name).cloned(),
                    DIVING_FISH_API_BASE_URL_ENV,
                    DEFAULT_DIVING_FISH_API_BASE_URL,
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn rejected_provider_credentials_are_not_rendered() -> Result<(), Box<dyn Error>> {
        let secret = "secret-sentinel";
        let values = HashMap::from([(
            DIVING_FISH_API_BASE_URL_ENV,
            OsString::from(format!("https://user:{secret}@example.test/api/")),
        )]);
        let result = provider_base_url(
            &|name| values.get(name).cloned(),
            DIVING_FISH_API_BASE_URL_ENV,
            DEFAULT_DIVING_FISH_API_BASE_URL,
        );
        assert!(result.is_err());
        let error = result.err().ok_or("provider credential URL was accepted")?;
        assert!(!format!("{error}").contains(secret));
        assert!(!format!("{error:?}").contains(secret));
        Ok(())
    }

    #[test]
    fn missing_cover_cache_path_is_kept_for_lazy_creation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("future-cover-cache");
        let values = HashMap::from([(COVER_DIR_ENVS[0], missing.clone().into_os_string())]);
        assert_eq!(
            resolved_path(
                temp.path(),
                None,
                &|name| values.get(name).cloned(),
                &COVER_DIR_ENVS
            ),
            Some(missing)
        );
        Ok(())
    }
}
