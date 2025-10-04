#![no_std]

extern crate alloc;

pub mod commands;
pub mod input;
pub mod output;
pub mod error;
pub mod types;
pub mod infrastructure;

#[cfg(test)]
mod tests;

pub use commands::CommandProcessor;
pub use input::InputHandler;
pub use output::OutputHandler;
pub use error::{ShellError, ShellResult};
pub use types::*;
pub use infrastructure::*;