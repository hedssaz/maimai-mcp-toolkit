use thiserror::Error;

#[derive(Debug, Error)]
pub enum MusicScoreError {
    #[error(transparent)]
    MusicResolution(#[from] crate::music_info::MusicInfoError),

    #[error(transparent)]
    Scores(#[from] crate::score_service::PlayerScoreServiceError),

    #[error("单曲成绩渲染任务异常终止")]
    TaskJoin,

    #[error(transparent)]
    Render(#[from] maimai_render::RenderError),

    #[error(transparent)]
    Output(#[from] crate::image_output::ImageOutputError),
}
