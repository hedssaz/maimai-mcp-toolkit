use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompletionError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("未找到牌子「{0}」的曲目数据")]
    UnknownPlate(String),
    #[error("批量完成表最多支持 50 项")]
    BatchTooLarge,
    #[error("页码超出范围，当前共计「{pages}」页")]
    PageOutOfRange { pages: usize },
    #[error("曲库查询失败：{0}")]
    Catalog(#[from] maimai_catalog::QueryError),
    #[error("成绩查询失败：{0}")]
    Scores(#[from] crate::score_service::PlayerScoreServiceError),
    #[error("图片渲染失败：{0}")]
    Render(#[from] maimai_render::RenderError),
    #[error("图片保存失败：{0}")]
    Output(#[from] crate::image_output::ImageOutputError),
    #[error("图片渲染任务意外退出")]
    TaskJoin,
}

impl CompletionError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}
