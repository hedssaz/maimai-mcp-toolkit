use thiserror::Error;

/// 在数据进入应用服务前即可确定的输入错误。
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ValidationError {
    #[error("{field} 不能为空")]
    Empty { field: &'static str },

    #[error("{field} 必须只包含 ASCII 数字")]
    NonNumeric { field: &'static str },

    #[error("歌曲 ID 不能为负数")]
    NegativeSongId,

    #[error("{field} 不能包含控制字符")]
    ControlCharacter { field: &'static str },

    #[error("谱面代数与难度组合无效")]
    InvalidChartKey,
}
