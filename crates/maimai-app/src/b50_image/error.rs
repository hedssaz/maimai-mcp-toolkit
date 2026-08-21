use std::{io, path::PathBuf};

use maimai_render::RenderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum B50ImageError {
    #[error("B50 图片风格配置无效：{path}")]
    InvalidStyleConfig { path: PathBuf },

    #[error("B50 图片状态路径不安全：{path}")]
    UnsafePath { path: PathBuf },

    #[error("B50 图片文件操作失败：{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("B50 图片输出策略无效：{message}")]
    InvalidOutputPolicy { message: &'static str },

    #[error("{field} 只允许使用进程启动时配置的目录。")]
    RuntimeOverrideUnsupported { field: &'static str },

    #[error("当前 static 目录缺少 Yuri-YuzuChaN/maimaiDX B50 模板资源，无法使用 yuzu 风格。")]
    YuzuAssetsRequired { missing: Vec<String> },

    #[error("当前 static 目录缺少 mai-bot B50 模板资源，无法使用 maibot 风格。")]
    MaibotAssetsRequired { missing: Vec<String> },

    #[error(transparent)]
    Render(RenderError),
}

impl B50ImageError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidStyleConfig { .. } => "STYLE_CONFIG_INVALID",
            Self::UnsafePath { .. } | Self::Io { .. } | Self::InvalidOutputPolicy { .. } => {
                "OUTPUT_STORE_ERROR"
            }
            Self::RuntimeOverrideUnsupported { .. } => "RUNTIME_OVERRIDE_UNSUPPORTED",
            Self::YuzuAssetsRequired { .. } => "YUZU_ASSETS_REQUIRED",
            Self::MaibotAssetsRequired { .. } => "MAIBOT_ASSETS_REQUIRED",
            Self::Render(_) => "RENDER_ERROR",
        }
    }

    pub fn missing_assets(&self) -> Option<&[String]> {
        match self {
            Self::YuzuAssetsRequired { missing } | Self::MaibotAssetsRequired { missing } => {
                Some(missing)
            }
            _ => None,
        }
    }

    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<RenderError> for B50ImageError {
    fn from(value: RenderError) -> Self {
        match value {
            RenderError::AssetsRequired {
                style: "yuzu",
                missing,
            } => Self::YuzuAssetsRequired { missing },
            RenderError::AssetsRequired {
                style: "maibot",
                missing,
            } => Self::MaibotAssetsRequired { missing },
            value => Self::Render(value),
        }
    }
}

impl From<crate::image_output::ImageOutputError> for B50ImageError {
    fn from(value: crate::image_output::ImageOutputError) -> Self {
        match value {
            crate::image_output::ImageOutputError::UnsafePath { path } => Self::UnsafePath { path },
            crate::image_output::ImageOutputError::Io { path, source } => Self::Io { path, source },
            crate::image_output::ImageOutputError::InvalidPolicy { message } => {
                Self::InvalidOutputPolicy { message }
            }
        }
    }
}
