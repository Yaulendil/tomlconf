use std::{
    ffi::OsString,
    fmt::{Display, Formatter, self},
    fs::{create_dir_all, File, rename},
    io::{Read, Seek, SeekFrom, Write},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};
use directories::ProjectDirs;
use serde::{de::DeserializeOwned, Serialize};


/// Locate the path of the configuration file.
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


/// Given a path, return a new path where a file at the first path may be moved
///     to save as a backup.
fn get_backup(path: &Path) -> Option<PathBuf> {
    const PREFIX: &str = ".bkp.";

    let name = path.file_name()?;
    let mut backup = OsString::with_capacity(PREFIX.len() + name.len());

    backup.push(PREFIX);
    backup.push(name);

    Some(path.with_file_name(backup))
}


/// The result of trying to find a configuration file.
pub enum ConfigFind<Cfg> {
    /// The file does not exist.
    DoesNotExist(PathBuf),
    /// The file does exist; Also includes the result of attempting to load it.
    Exists(PathBuf, ConfigOpen<Cfg>),
    /// No path was found at which to search for a file.
    NoPath,
}

impl<Cfg> ConfigFind<Cfg> {
    /// Get a reference to the configuration inside this value, if it was opened
    ///     successfully.
    pub fn config(&self) -> Option<&Cfg> {
        match self {
            Self::Exists(_, open) => open.config(),
            _ => None,
        }
    }

    /// Get the configuration inside this value, if it was opened successfully.
    pub fn into_config(self) -> Option<Cfg> {
        match self {
            Self::Exists(_, open) => open.into_config(),
            _ => None,
        }
    }

    /// Get the configuration inside this value, if it was opened successfully.
    pub fn into_result(self) -> Result<ConfigOpen<Cfg>, Self> {
        match self {
            Self::Exists(_, open) => Ok(open),
            err => Err(err),
        }
    }

    /// Return a reference to the filepath checked by the search operation, if
    ///     there was one.
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


/// The result of attempting to load a configuration from a filepath.
pub enum ConfigOpen<Cfg> {
    /// The file could not be opened.
    FileInaccessible(std::io::Error),
    /// The file could not be parsed.
    FileInvalid(toml::de::Error),
    /// The file load was successful.
    FileValid(Cfg),
}

impl<Cfg> ConfigOpen<Cfg> {
    /// Get a reference to the configuration inside this value, if it was opened
    ///     successfully.
    pub fn config(&self) -> Option<&Cfg> {
        match self {
            Self::FileValid(config) => Some(config),
            _ => None,
        }
    }

    /// Get the configuration inside this value, if it was opened successfully.
    pub fn into_config(self) -> Option<Cfg> {
        match self {
            Self::FileValid(config) => Some(config),
            _ => None,
        }
    }

    /// Get the configuration inside this value, if it was opened successfully.
    pub fn into_result(self) -> Result<Cfg, Self> {
        match self {
            Self::FileValid(config) => Ok(config),
            err => Err(err),
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


/// An error returned when attempting to save a configuration into a file.
#[derive(Debug)]
pub enum ConfigSaveError {
    /// The file could not be opened.
    FileInaccessible(std::io::Error),
    /// The data could not be converted.
    SerializeFailure(toml::ser::Error),
}


impl From<std::io::Error> for ConfigSaveError {
    fn from(e: std::io::Error) -> Self { Self::FileInaccessible(e) }
}


impl From<toml::ser::Error> for ConfigSaveError {
    fn from(e: toml::ser::Error) -> Self { Self::SerializeFailure(e) }
}


/// Implements a set of convenience functions for finding a configuration file
///     and deserializing it into a usable struct.
pub trait ConfigData: DeserializeOwned {
    /// The text data of a default configuration file, as UTF-8. Ideally read
    ///     from a default file at compile time using the [`include_str`] macro.
    const DEFAULT: &'static str;

    /// Create a default configuration file at the given path.
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
                    create_dir_all(parent)?;
                }
            }
        }

        File::create(path)?.write_all(Self::DEFAULT.as_bytes())
    }

    /// Load the default configuration directly, without looking for a file.
    fn default() -> Result<Self, toml::de::Error> {
        let new: Self = toml::from_str(Self::DEFAULT)?;

        Ok(new.prepare())
    }

    /// Find and read a configuration file from a path defined programmatically
    ///     by [`ProjectDirs`].
    ///
    /// # Arguments
    ///
    /// The first three of these arguments will be passed directly to
    ///     [`ProjectDirs::from`]. Refer to that function for more details.
    ///
    /// * `qualifier`: Reverse domain qualifier. May be empty (`""`).
    /// * `organization`: The organization responsible for the application. May
    ///     be empty.
    /// * `application`: The name of the application itself.
    /// * `file`: The filename to be used for the Configuration file. This file
    ///     will be placed into the directory returned by calling [`config_dir`]
    ///     on the [`ProjectDirs`] created from the previous arguments.
    ///
    /// returns: `ConfigFind<Self>`
    ///
    /// [`config_dir`]: ProjectDirs::config_dir
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

    /// Read a new configuration from a specific file, if it exists.
    fn from_path(path: PathBuf) -> ConfigFind<Self> {
        if path.exists() {
            let open = Self::open(&path);
            ConfigFind::Exists(path, open)
        } else {
            ConfigFind::DoesNotExist(path)
        }
    }

    /// Read a new configuration from a specific filepath, without first
    ///     confirming that the file exists.
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

    /// Attempt to automatically handle all aspects of configuration setup.
    ///
    /// If any step of the process fails, this function returns a description
    ///     stating plainly what went wrong, suitable for immediate delivery to
    ///     the user. If the process is successful, the return is a string
    ///     describing the source of the configuration (either opening a found
    ///     file or creating a new one), as well as the configuration structure
    ///     itself.
    ///
    /// Arguments passed to this function are the same as those of [`find`].
    ///
    /// [`find`]: Self::find
    fn setup(
        qualifier: &str,
        organization: &str,
        application: &str,
        file: &str,
    ) -> Result<(String, ConfigFile<Self>), String> {
        use ConfigFind::*;

        match Self::find(qualifier, organization, application, file) {
            DoesNotExist(path) => match Self::create(&path, true, true) {
                Err(e) => Err(format!(
                    "Cannot save {} as Config file: {}",
                    path.display(), e,
                )),
                Ok(..) => match Self::open(&path) {
                    ConfigOpen::FileInaccessible(e) => Err(format!(
                        "Cannot access {} as Config file: {}",
                        path.display(), e,
                    )),
                    ConfigOpen::FileInvalid(e) => Err(format!(
                        "Cannot read {} as Config file: {}",
                        path.display(), e,
                    )),
                    ConfigOpen::FileValid(cfg) => Ok((
                        format!("Created new Config file: {}", path.display()),
                        cfg.with_path(path),
                    )),
                }
            }
            Exists(path, cfg) => match cfg {
                ConfigOpen::FileInaccessible(e) => Err(format!(
                    "Cannot access {} as Config file: {}",
                    path.display(), e,
                )),
                ConfigOpen::FileInvalid(e) => Err(format!(
                    "Cannot read {} as Config file: {}",
                    path.display(), e,
                )),
                ConfigOpen::FileValid(cfg) => Ok((
                    format!("Using existing Config file: {}", path.display()),
                    cfg.with_path(path),
                )),
            }
            NoPath => Err(String::from("Cannot find path for Config file.")),
        }
    }

    /// Attempt to automatically handle all aspects of configuration setup.
    ///
    /// If the target file exists, but is invalid, it will be overwritten with a
    ///     new default file.
    ///
    /// Arguments passed to this function are the same as those of [`find`].
    ///
    /// [`find`]: Self::find
    fn setup_replace_invalid(
        qualifier: &str,
        organization: &str,
        application: &str,
        file: &str,
    ) -> Result<(String, ConfigFile<Self>), String> {
        use ConfigFind::*;

        match Self::find(qualifier, organization, application, file) {
            Exists(path, ConfigOpen::FileInvalid(_))
            | DoesNotExist(path) => match Self::create(&path, true, true) {
                Err(e) => Err(format!(
                    "Cannot save {} as Config file: {}",
                    path.display(), e,
                )),
                Ok(..) => match Self::open(&path) {
                    ConfigOpen::FileInaccessible(e) => Err(format!(
                        "Cannot access {} as Config file: {}",
                        path.display(), e,
                    )),
                    ConfigOpen::FileInvalid(e) => Err(format!(
                        "Cannot read {} as Config file: {}",
                        path.display(), e,
                    )),
                    ConfigOpen::FileValid(cfg) => Ok((
                        format!("Created new Config file: {}", path.display()),
                        cfg.with_path(path),
                    )),
                }
            }
            Exists(path, cfg) => match cfg {
                ConfigOpen::FileInaccessible(e) => Err(format!(
                    "Cannot access {} as Config file: {}",
                    path.display(), e,
                )),
                ConfigOpen::FileInvalid(_) => unreachable!(),
                ConfigOpen::FileValid(cfg) => Ok((
                    format!("Using existing Config file: {}", path.display()),
                    cfg.with_path(path),
                )),
            }
            NoPath => Err(String::from("Cannot find path for Config file.")),
        }
    }
}


/// A pairing of a configuration state with the file path at which it is saved.
#[derive(Clone)]
pub struct ConfigFile<Cfg> {
    /// The stored configuration state.
    pub data: Cfg,
    /// The path to the file associated with this configuration state.
    pub path: PathBuf,
}


impl<Cfg: ConfigData> ConfigFile<Cfg> {
    /// Read from the file associated with this configuration, and replace the
    ///     stored data.
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

    /// Read from the file associated with this configuration, and replace the
    ///     stored data.
    #[cfg(feature = "nightly")]
    pub fn reload(&mut self) -> Result<(), ConfigOpen<Cfg>> {
        self.data = Cfg::open(&self.path)?;
        Ok(())

        // Ok(std::mem::replace(&mut self.data, Cfg::open(&self.path)?))
    }

    /// Write the configuration into a new file at the associated path.
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
                    create_dir_all(parent)?;
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
