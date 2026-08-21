use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageOutputError {
    #[error("图片输出路径不安全：{path}")]
    UnsafePath { path: PathBuf },

    #[error("图片文件操作失败：{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("图片输出策略无效：{message}")]
    InvalidPolicy { message: &'static str },
}

impl ImageOutputError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
