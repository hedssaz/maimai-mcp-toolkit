use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use maimai_render::{
    B50View, CoverResolver, LegacyRenderer, MaibotRenderer, RenderedPng, YuzuRenderer,
};
use time::OffsetDateTime;

use super::{
    B50ImageError, B50ImageStyle, OutputStore, StyleSelection, StyleStore, StyleUpdate,
    atomic_file::secure_absolute,
};

pub struct B50ImageService {
    styles: StyleStore,
    outputs: OutputStore,
    renderer: LegacyRenderer,
    yuzu: Option<YuzuRenderer>,
    maibot: Option<MaibotRenderer>,
    directories: ResourceDirectories,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDirectories {
    pub static_dir: PathBuf,
    pub cover_cache_dir: PathBuf,
    yuzu_static_dir: Option<PathBuf>,
    maibot_static_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderOptions {
    pub style: Option<B50ImageStyle>,
    pub output_dir: Option<PathBuf>,
    pub static_dir: Option<PathBuf>,
    pub cover_cache_dir: Option<PathBuf>,
    pub filename_stem: String,
}

#[derive(Debug)]
pub struct B50RenderResult {
    pub rendered: RenderedPng,
    pub image_path: PathBuf,
    pub style: B50ImageStyle,
    pub draw_elapsed: Duration,
    pub save_elapsed: Duration,
}

impl ResourceDirectories {
    pub fn new(
        static_dir: impl Into<PathBuf>,
        cover_cache_dir: impl Into<PathBuf>,
    ) -> Result<Self, B50ImageError> {
        Ok(Self {
            static_dir: secure_absolute(static_dir.into())?,
            cover_cache_dir: secure_absolute(cover_cache_dir.into())?,
            yuzu_static_dir: None,
            maibot_static_dir: None,
        })
    }

    pub fn with_style_static(
        mut self,
        style: B50ImageStyle,
        path: impl Into<PathBuf>,
    ) -> Result<Self, B50ImageError> {
        let path = secure_absolute(path.into())?;
        match style {
            B50ImageStyle::Legacy => self.static_dir = path,
            B50ImageStyle::Yuzu => self.yuzu_static_dir = Some(path),
            B50ImageStyle::Maibot => self.maibot_static_dir = Some(path),
        }
        Ok(self)
    }

    fn static_for(&self, style: B50ImageStyle) -> PathBuf {
        match style {
            B50ImageStyle::Yuzu => self.yuzu_static_dir.as_ref(),
            B50ImageStyle::Maibot => self.maibot_static_dir.as_ref(),
            B50ImageStyle::Legacy => None,
        }
        .unwrap_or(&self.static_dir)
        .clone()
    }
}

impl B50ImageService {
    pub fn new(
        styles: StyleStore,
        outputs: OutputStore,
        renderer: LegacyRenderer,
        directories: ResourceDirectories,
    ) -> Self {
        let yuzu = YuzuRenderer::new(directories.static_for(B50ImageStyle::Yuzu)).ok();
        let maibot =
            MaibotRenderer::new(directories.static_for(B50ImageStyle::Maibot), &renderer).ok();
        Self {
            styles,
            outputs,
            renderer,
            yuzu,
            maibot,
            directories,
        }
    }

    pub fn current_style(&self) -> Result<StyleSelection, B50ImageError> {
        self.styles.current()
    }

    pub fn set_style(
        &self,
        style: B50ImageStyle,
        now: OffsetDateTime,
    ) -> Result<StyleUpdate, B50ImageError> {
        self.styles.set(style, now)
    }

    pub fn render(
        &self,
        view: &B50View,
        options: RenderOptions,
        now: OffsetDateTime,
    ) -> Result<B50RenderResult, B50ImageError> {
        let style = match options.style {
            Some(style) => style,
            None => self.styles.current()?.style,
        };
        validate_override("outputDir", options.output_dir, self.outputs.root())?;
        let outputs = self.outputs.clone();
        let static_dir = self.directories.static_for(style);
        validate_override("staticDir", options.static_dir, &static_dir)?;
        let cover_cache_dir = self.directories.cover_cache_dir.clone();
        validate_override("coverCacheDir", options.cover_cache_dir, &cover_cache_dir)?;
        let covers = CoverResolver::new(&static_dir, &cover_cache_dir);
        let draw_started = std::time::Instant::now();
        let rendered = match style {
            B50ImageStyle::Legacy => self.renderer.render_with_covers(view, &covers)?,
            B50ImageStyle::Yuzu => match &self.yuzu {
                Some(renderer) => renderer.render(view, &covers)?,
                None => YuzuRenderer::new(&static_dir)?.render(view, &covers)?,
            },
            B50ImageStyle::Maibot => match &self.maibot {
                Some(renderer) => renderer.render(view, &covers)?,
                None => MaibotRenderer::new(&static_dir, &self.renderer)?.render(view, &covers)?,
            },
        };
        let draw_elapsed = draw_started.elapsed();
        let save_started = std::time::Instant::now();
        let saved = outputs.save_png(&options.filename_stem, &rendered.bytes, now)?;
        let save_elapsed = save_started.elapsed();
        Ok(B50RenderResult {
            rendered,
            image_path: saved.path,
            style,
            draw_elapsed,
            save_elapsed,
        })
    }
}

fn validate_override(
    field: &'static str,
    requested: Option<PathBuf>,
    configured: &Path,
) -> Result<(), B50ImageError> {
    let Some(requested) = requested else {
        return Ok(());
    };
    if secure_absolute(requested)? == configured {
        return Ok(());
    }
    Err(B50ImageError::RuntimeOverrideUnsupported { field })
}
