use std::{
    ffi::OsString,
    fmt::{Display, Formatter, self},
    fs::{create_dir, File, rename},
    io::{Read, Seek, SeekFrom, Write},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};
use directories::ProjectDirs;
use serde::{de::DeserializeOwned, Serialize};


/// Locate the Path of the Config File.
fn find_path(
    qualifier: &str,
    organization: &str,
    application: &str,
    filename: &str,
) -> Option<PathBuf> {
    let dirs = ProjectDirs::from(qualifier, organization, application)?;
    let mut path: PathBuf = dirs.config_dir().to_owned();

    path.push(filename);
    Some(path)
}


fn get_backup(path: &Path) -> Option<PathBuf> {
    const PREFIX: &str = ".bkp.";

    let name = path.file_name()?;
    let mut backup = OsString::with_capacity(PREFIX.len() + name.len());

    backup.push(PREFIX);
    backup.push(name);

    Some(path.with_file_name(backup))
}


pub enum ConfigFind<Cfg> {
    DoesNotExist(PathBuf),
    Exists(PathBuf, ConfigOpen<Cfg>),
    NoPath,
}

impl<Cfg> ConfigFind<Cfg> {
    /// Get a reference to the config data inside this value, if it was opened
    ///     successfully.
    pub fn config(&self) -> Option<&Cfg> {
        match self {
            Self::Exists(_, open) => open.config(),
            _ => None,
        }
    }

    /// Get the config data inside this value, if it was opened successfully.
    pub fn into_config(self) -> Option<Cfg> {
        match self {
            Self::Exists(_, open) => open.into_config(),
            _ => None,
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::NoPath => None,
            Self::Exists(path, _)
            | Self::DoesNotExist(path) => Some(path),
        }
    }
}


impl<Cfg> Display for ConfigFind<Cfg> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DoesNotExist(path) => {
                write!(f, "File does not exist: {}", path.display())
            }
            Self::Exists(path, open) => {
                write!(f, "{} at {}", open, path.display())
            }
            Self::NoPath => f.write_str("Cannot find config path."),
        }
    }
}


pub enum ConfigOpen<Cfg> {
    FileInaccessible(std::io::Error),
    FileInvalid(toml::de::Error),
    FileValid(Cfg),
}

impl<Cfg> ConfigOpen<Cfg> {
    /// Get a reference to the config data inside this value, if it was opened
    ///     successfully.
    pub fn config(&self) -> Option<&Cfg> {
        match self {
            Self::FileValid(config) => Some(config),
            _ => None,
        }
    }

    /// Get the config data inside this value, if it was opened successfully.
    pub fn into_config(self) -> Option<Cfg> {
        match self {
            Self::FileValid(config) => Some(config),
            _ => None,
        }
    }
}


impl<Cfg> Display for ConfigOpen<Cfg> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileInaccessible(e) => {
                write!(f, "Cannot access file: {}", e)
            }
            Self::FileInvalid(e) => {
                write!(f, "Cannot read configuration: {}", e)
            }
            Self::FileValid(..) => f.write_str("Successfully read file"),
        }
    }
}


#[cfg(feature = "nightly")]
impl<Cfg> std::ops::FromResidual for ConfigOpen<Cfg> {
    fn from_residual(residual: <Self as std::ops::Try>::Residual) -> Self {
        match residual {
            Err(e) => e,
            Ok(..) => unreachable!(),
        }
    }
}


#[cfg(feature = "nightly")]
impl<Cfg> std::ops::Try for ConfigOpen<Cfg> {
    type Output = Cfg;
    type Residual = Result<std::convert::Infallible, Self>;

    fn from_output(output: Self::Output) -> Self {
        Self::FileValid(output)
    }

    fn branch(self) -> std::ops::ControlFlow<Self::Residual, Self::Output> {
        use std::ops::ControlFlow;

        match self {
            Self::FileValid(cfg) => ControlFlow::Continue(cfg),
            other => ControlFlow::Break(Err(other)),
        }
    }
}


pub enum ConfigSaveError {
    FileInaccessible(std::io::Error),
    SerializeFailure(toml::ser::Error),
}


impl From<std::io::Error> for ConfigSaveError {
    fn from(e: std::io::Error) -> Self { Self::FileInaccessible(e) }
}


impl From<toml::ser::Error> for ConfigSaveError {
    fn from(e: toml::ser::Error) -> Self { Self::SerializeFailure(e) }
}


pub trait ConfigData: DeserializeOwned {
    const DEFAULT: &'static str;

    /// Create a default Configuration file at the given path.
    ///
    /// # Arguments
    ///
    /// * `path`: The path at which to create the new file.
    /// * `create_backup`: Whether to try to save a backup of the current file,
    ///     if it already exists.
    /// * `create_parent`: Whether to try to create the parent directory for the
    ///     new file, if it does not exist.
    ///
    /// returns: `Result<(), std::io::Error>`
    fn create(
        path: &Path,
        create_backup: bool,
        create_parent: bool,
    ) -> Result<(), std::io::Error> {
        if create_backup && path.exists() {
            if let Some(backup) = get_backup(path) {
                rename(path, backup).ok();
            }
        } else if create_parent {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    create_dir(parent)?;
                }
            }
        }

        File::create(path)?.write_all(Self::DEFAULT.as_bytes())
    }

    fn find(
        qualifier: &str,
        organization: &str,
        application: &str,
        file: &str,
    ) -> ConfigFind<Self> {
        match find_path(qualifier, organization, application, file) {
            None => ConfigFind::NoPath,
            Some(path) if !path.exists() => ConfigFind::DoesNotExist(path),
            Some(path) => {
                let open = Self::open(&path);
                ConfigFind::Exists(path, open)
            }
        }
    }

    fn from_path(path: PathBuf) -> ConfigFind<Self> {
        if path.exists() {
            let open = Self::open(&path);

            ConfigFind::Exists(path, open)
        } else {
            ConfigFind::DoesNotExist(path)
        }
    }

    fn from_path_or_auto(
        path_opt: Option<PathBuf>,
        qualifier: &str,
        organization: &str,
        application: &str,
        file: &str,
    ) -> ConfigFind<Self> {
        match path_opt {
            None => Self::find(qualifier, organization, application, file),
            Some(path) => Self::from_path(path),
        }
    }

    fn open(path: &Path) -> ConfigOpen<Self> {
        use ConfigOpen::*;

        let data = match File::open(&path) {
            Ok(mut file) => match file.seek(SeekFrom::End(0)) {
                Ok(len) => match file.rewind() {
                    Ok(..) => {
                        let mut buf = String::with_capacity(1 + len as usize);

                        match file.read_to_string(&mut buf) {
                            Ok(..) => buf,
                            Err(e) => { return FileInaccessible(e); }
                        }
                    }
                    Err(e) => { return FileInaccessible(e); }
                }
                Err(e) => { return FileInaccessible(e); }
            }
            Err(e) => { return FileInaccessible(e); }
        };

        match toml::from_str::<Self>(&data) {
            Err(e) => FileInvalid(e),
            Ok(config) => FileValid(config.prepare()),
        }
    }

    /// Perform any final transformations that should be done on a newly parsed
    ///     configuration, such as enforcing specific casing for string fields.
    ///
    /// This is a no-op by default, and is intended to be overridden.
    fn prepare(self) -> Self { self }

    /// Associate a file path with this configuration.
    fn with_path(self, path: PathBuf) -> ConfigFile<Self> {
        ConfigFile { data: self, path }
    }
}


#[derive(Clone)]
pub struct ConfigFile<Cfg> {
    /// The stored configuration state.
    pub data: Cfg,
    /// The path to the file associated with this configuration state.
    pub path: PathBuf,
}


impl<Cfg: ConfigData> ConfigFile<Cfg> {
    #[cfg(not(feature = "nightly"))]
    pub fn reload(&mut self) -> Result<(), ConfigOpen<Cfg>> {
        // use std::mem::replace;

        match Cfg::open(&self.path) {
            // ConfigOpen::FileValid(new) => Ok(replace(&mut self.data, new)),
            ConfigOpen::FileValid(new) => {
                self.data = new;
                Ok(())
            }
            err => Err(err),
        }
    }

    #[cfg(feature = "nightly")]
    pub fn reload(&mut self) -> Result<(), ConfigOpen<Cfg>> {
        self.data = Cfg::open(&self.path)?;
        Ok(())

        // Ok(std::mem::replace(&mut self.data, Cfg::open(&self.path)?))
    }

    pub fn save(
        &self,
        create_backup: bool,
        create_parent: bool,
    ) -> Result<(), ConfigSaveError>
        where Cfg: Serialize
    {
        let Self { data, path } = self;

        if create_backup && path.exists() {
            if let Some(backup) = get_backup(path) {
                rename(path, backup).ok();
            }
        } else if create_parent {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    create_dir(parent)?;
                }
            }
        }

        let serial: String = toml::to_string(data)?;
        Ok(File::create(path)?.write_all(serial.as_bytes())?)
    }
}


impl<Cfg> Deref for ConfigFile<Cfg> {
    type Target = Cfg;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}


impl<Cfg> DerefMut for ConfigFile<Cfg> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
