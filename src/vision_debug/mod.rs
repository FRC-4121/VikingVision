//! Utilities to debug video streams
#![allow(clippy::new_ret_no_self)]

use crate::buffer::Buffer;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use std::sync::OnceLock;

mod dispatch;

#[cfg(feature = "debug-gui")]
mod with_winit;
mod without_winit;

#[cfg(feature = "debug-gui")]
use with_winit as backend;
#[cfg(not(feature = "debug-gui"))]
use without_winit as backend;

pub use backend::Handler;
use backend::Signal;

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
                Ok(title) if !title.is_empty() => self.default_title = title,
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

struct ById(DebugImage);
impl PartialEq for ById {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for ById {}
impl Hash for ById {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

/// A message to be handled by the debug handler.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    HasImage,
    Shutdown,
}

pub static GLOBAL_SENDER: OnceLock<Sender> = OnceLock::new();

pub struct Sender {
    signal: Signal,
    writer: dispatch::Writer,
}
impl Sender {
    pub fn send_image(&self, message: DebugImage) {
        if self.writer.write(message) {
            self.signal.send(Message::HasImage);
        }
    }
    pub fn shutdown(&self) {
        self.signal.send(Message::Shutdown);
    }
}

/// A pair of the handler and a sender.
pub struct HandlerWithSender {
    pub handler: Handler,
    pub sender: Sender,
}
impl HandlerWithSender {
    pub fn init_global_sender(self) -> Handler {
        if GLOBAL_SENDER.set(self.sender).is_err() {
            tracing::warn!("global sender is already set");
        }
        self.handler
    }
}
