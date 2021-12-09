//! TomlConf uses the [`directories`] library to locate the appropriate place
//!     for application data in a cross-platform way, and populates that
//!     location with a default file included at compile-time.
//!
//! All you need to do is define a struct that implements [`DeserializeOwned`]
//!     (typically by way of `#[derive(Deserialize)]` for a struct that owns its
//!     data) and implement the [`ConfigData`] trait for it. You can then use
//!     the constructors on the trait to create, load, and read the data to a
//!     file; If you also derive [`Serialize`], you can even save changes to the
//!     data back into the file.
//!
//! [`DeserializeOwned`]: serde::de::DeserializeOwned
//! [`Serialize`]: serde::Serialize
//!
//! # Example
//!
//! ```
//! use tomlconf::*;
//!
//!
//! #[derive(Deserialize)]
//! struct AppConfig {
//!     output: String,
//!     number: usize,
//! }
//!
//! impl ConfigData for AppConfig {
//!     const DEFAULT: &'static str = include_str!("cfg_default.toml");
//! }
//!
//!
//! fn main() {
//!     let cfg: ConfigFile<AppConfig> = match AppConfig::setup(
//!         "com", // "Qualifier"; OSX-specific.
//!         "Cool Software LTD", // Organization name.
//!         "TextPrinter", // Application name.
//!         "config.toml", // Configuration file name.
//!     ).unwrap() {
//!         Ok((msg, config)) => {
//!             //  This `msg` variable tells the user whether an existing config
//!             //      file was found, or whether a new one was created with the
//!             //      default values instead.
//!             eprintln!("{}", msg);
//!             config
//!         }
//!         Err(msg) => {
//!             eprintln!("Setup failed: {}", msg);
//!             std::process::exit(1);
//!         }
//!     };
//!
//!     for i in 0..cfg.number {
//!         println!("{}: {}", i, &cfg.output);
//!     }
//! }
//! ```

#![cfg_attr(feature = "nightly", feature(try_trait_v2))]

mod config;

pub use config::*;
