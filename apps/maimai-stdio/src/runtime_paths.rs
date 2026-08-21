use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

const DATA_DIR_ENV: &str = "MAIMAI_DATA_DIR";
const STATE_DB_ENV: &str = "MAIMAI_STATE_DB";
const BINDINGS_DB_ENV: &str = "MAIMAI_BINDINGS_DB_PATH";
const LOCAL_DB_ENV: &str = "MAIMAI_LOCAL_DB_PATH";
const HOME_ENV: &str = "HOME";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimePaths {
    data_dir: PathBuf,
    state_db: PathBuf,
}

impl RuntimePaths {
    pub(crate) fn from_env() -> Result<Self, RuntimePathError> {
        let cwd = env::current_dir().map_err(RuntimePathError::CurrentDirectory)?;
        Self::resolve(&cwd, |name| env::var_os(name))
    }

    pub(crate) fn resolve(
        cwd: &Path,
        get: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, RuntimePathError> {
        let home = non_empty(get(HOME_ENV)).map(PathBuf::from);
        let data_input = non_empty(get(DATA_DIR_ENV)).unwrap_or_else(|| OsString::from("data"));
        let data_dir = configured_path(cwd, Path::new(&data_input), home.as_deref());
        let data_dir =
            data_dir
                .canonicalize()
                .map_err(|source| RuntimePathError::DataDirectory {
                    path: data_dir,
                    source,
                })?;
        if !data_dir.is_dir() {
            return Err(RuntimePathError::DataDirectoryNotDirectory(data_dir));
        }

        let state_db = resolve_state_db(cwd, &data_dir, home.as_deref(), &get)?;

        Ok(Self { data_dir, state_db })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn state_db(&self) -> &Path {
        &self.state_db
    }
}

fn resolve_state_db(
    cwd: &Path,
    default_dir: &Path,
    home: Option<&Path>,
    get: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, RuntimePathError> {
    match non_empty(get(STATE_DB_ENV)) {
        Some(path) => Ok(configured_path(cwd, Path::new(&path), home)),
        None => {
            let bindings = non_empty(get(BINDINGS_DB_ENV))
                .map(|path| configured_path(cwd, Path::new(&path), home));
            let local = non_empty(get(LOCAL_DB_ENV))
                .map(|path| configured_path(cwd, Path::new(&path), home));
            match (bindings, local) {
                (Some(bindings), Some(local)) if bindings != local => {
                    Err(RuntimePathError::ConflictingLegacyStatePaths { bindings, local })
                }
                (Some(path), _) | (_, Some(path)) => Ok(path),
                (None, None) => Ok(default_dir.join("maimai-local.db")),
            }
        }
    }
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

pub(crate) fn configured_path(cwd: &Path, path: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = expand_home(path, home);
    absolute(cwd, &expanded)
}

fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let mut components = path.components();
    match (components.next(), home) {
        (Some(Component::Normal(part)), Some(home)) if part == OsStr::new("~") => {
            home.join(components.as_path())
        }
        _ => path.to_owned(),
    }
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    }
}

#[derive(Debug, Error)]
pub enum RuntimePathError {
    #[error("读取当前工作目录失败：{0}")]
    CurrentDirectory(io::Error),

    #[error("数据目录 {path} 不可用：{source}")]
    DataDirectory { path: PathBuf, source: io::Error },

    #[error("数据目录不是文件夹：{0}")]
    DataDirectoryNotDirectory(PathBuf),

    #[error("MAIMAI_BINDINGS_DB_PATH 与 MAIMAI_LOCAL_DB_PATH 指向不同数据库：{bindings} / {local}")]
    ConflictingLegacyStatePaths { bindings: PathBuf, local: PathBuf },
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, error::Error, fs};

    use super::*;

    #[test]
    fn defaults_are_rooted_at_current_directory() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("data"))?;
        let paths = RuntimePaths::resolve(temp.path(), |_| None)?;
        assert_eq!(paths.data_dir(), temp.path().join("data").canonicalize()?);
        assert_eq!(
            paths.state_db(),
            temp.path()
                .join("data")
                .canonicalize()?
                .join("maimai-local.db")
        );
        Ok(())
    }

    #[test]
    fn primary_state_path_overrides_legacy_paths() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("catalog"))?;
        let home = temp.path().join("home");
        let values = HashMap::from([
            (DATA_DIR_ENV, OsString::from("catalog")),
            (STATE_DB_ENV, OsString::from("~/primary.db")),
            (BINDINGS_DB_ENV, OsString::from("legacy-a.db")),
            (LOCAL_DB_ENV, OsString::from("legacy-b.db")),
            (HOME_ENV, home.clone().into_os_string()),
        ]);
        let paths = RuntimePaths::resolve(temp.path(), |name| values.get(name).cloned())?;
        assert_eq!(paths.state_db(), home.join("primary.db"));
        Ok(())
    }

    #[test]
    fn distinct_legacy_state_paths_are_rejected() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("data"))?;
        let values = HashMap::from([
            (BINDINGS_DB_ENV, OsString::from("a.db")),
            (LOCAL_DB_ENV, OsString::from("b.db")),
        ]);
        let error = match RuntimePaths::resolve(temp.path(), |name| values.get(name).cloned()) {
            Ok(_) => return Err("conflicting legacy paths were accepted".into()),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            RuntimePathError::ConflictingLegacyStatePaths { .. }
        ));
        Ok(())
    }

    #[test]
    fn empty_values_use_defaults() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("data"))?;
        let values = HashMap::from([
            (DATA_DIR_ENV, OsString::new()),
            (STATE_DB_ENV, OsString::new()),
        ]);
        let paths = RuntimePaths::resolve(temp.path(), |name| values.get(name).cloned())?;
        assert_eq!(paths.state_db(), paths.data_dir().join("maimai-local.db"));
        Ok(())
    }

    #[test]
    fn non_directory_data_path_is_rejected() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("catalog.json");
        fs::write(&file, b"{}")?;
        let values = HashMap::from([(DATA_DIR_ENV, file.into_os_string())]);
        let error = match RuntimePaths::resolve(temp.path(), |name| values.get(name).cloned()) {
            Ok(_) => return Err("a file was accepted as a data directory".into()),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            RuntimePathError::DataDirectoryNotDirectory(_)
        ));
        Ok(())
    }

    #[test]
    fn state_resolution_does_not_require_the_default_data_directory() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let state = resolve_state_db(temp.path(), &temp.path().join("data"), None, &|_| None)?;
        assert_eq!(state, temp.path().join("data/maimai-local.db"));
        assert!(!temp.path().join("data").exists());
        Ok(())
    }
}
