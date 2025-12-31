//! Utilities to debug video streams

use crate::buffer::Buffer;
use serde::{Deserialize, Serialize};

mod without_winit;

pub use without_winit::{Handler, Sender};

/// What to do with the image we received.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum DebugMode {
    /// Auto-select based on global configuration.
    #[default]
    Auto,
    /// Ignore it.
    None,
    /// Save the image.
    Save {
        path: Option<String>,
    },
    Show {
        title: Option<String>,
    },
}
impl From<DefaultDebugMode> for DebugMode {
    fn from(value: DefaultDebugMode) -> Self {
        match value {
            DefaultDebugMode::None => Self::None,
            DefaultDebugMode::Save => Self::Save { path: None },
            DefaultDebugMode::Show => Self::Show { title: None },
        }
    }
}

/// How we should handle images that are sent with [`DebugMode::Auto`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultDebugMode {
    #[default]
    None,
    Save,
    Show,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefaultDebug {
    pub mode: Option<DefaultDebugMode>,
    #[serde(default)]
    pub default_path: String,
    #[serde(default)]
    pub default_title: String,
}
impl DefaultDebug {
    pub fn from_env() -> Self {
        let mut out = Self::default();
        out.update_from_env();
        out
    }
    pub fn update_from_env(&mut self) {
        if self.mode.is_none() {
            match std::env::var("VV_DEBUG_MODE").as_deref() {
                Ok("none" | "NONE") => self.mode = Some(DefaultDebugMode::None),
                Ok("save" | "SAVE") => self.mode = Some(DefaultDebugMode::Save),
                Ok("show" | "SHOW") => self.mode = Some(DefaultDebugMode::Show),
                Ok(mode) => tracing::error!(mode, "unknown value for VV_DEBUG_MODE"),
                Err(std::env::VarError::NotUnicode(mode)) => {
                    tracing::error!(?mode, "unknown value for VV_DEBUG_MODE")
                }
                Err(_) => tracing::info!("no value for VV_DEBUG_MODE set, defaulting to \"none\""),
            }
        } else {
            tracing::info!("not checking VV_DEBUG_MODE because a default mode is already set");
        }
        if self.default_path.is_empty() {
            match std::env::var("VV_DEBUG_SAVE_PATH") {
                Ok(path) if !path.is_empty() => self.default_path = path,
                Err(std::env::VarError::NotUnicode(path)) => {
                    tracing::error!(?path, "the value of VV_DEBUG_SAVE_PATH must be valid UTF-8")
                }
                _ => tracing::info!("no value for VV_DEBUG_SAVE_PATH set"),
            }
        } else {
            tracing::info!("not checking VV_DEBUG_SAVE_PATH because a default path is already set");
        }
        if self.default_title.is_empty() {
            match std::env::var("VV_DEBUG_WINDOW_TITLE") {
                Ok(path) if !path.is_empty() => self.default_title = path,
                Err(std::env::VarError::NotUnicode(path)) => {
                    tracing::error!(
                        ?path,
                        "the value of VV_DEBUG_WINDOW_TITLE must be valid UTF-8"
                    )
                }
                _ => tracing::info!("no value for VV_DEBUG_WINDOW_TITLE set"),
            }
        } else {
            tracing::info!(
                "not checking VV_DEBUG_WINDOW_TITLE because a default title is already set"
            );
        }
    }
}

/// An image to be debugged.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugImage {
    /// The image to show.
    pub image: Buffer<'static>,
    /// A pretty, human-readable name
    pub name: String,
    /// A unique identifier for this image, that should not change.
    pub id: u128,
    /// The mode we want to use to debug this image.
    pub mode: DebugMode,
}

/// A message to be handled by the debug handler.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    DebugImage(DebugImage),
    Shutdown,
}
