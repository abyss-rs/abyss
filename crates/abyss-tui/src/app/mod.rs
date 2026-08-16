pub(crate) mod actions;
pub(crate) mod dialogs;
mod event_loop;
mod input;
pub(crate) mod menu;
mod runner;
pub(crate) mod state;
mod status;
pub(crate) mod sync;

#[cfg(test)]
mod tests;

pub use runner::run;

pub(crate) use self::dialogs::*;
pub(crate) use self::menu::*;
pub(crate) use self::state::*;
pub(crate) use self::sync::*;
