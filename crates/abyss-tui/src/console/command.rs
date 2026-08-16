use std::path::Path;

/// A `cd` line for the shell that stays out of shell history.
///
/// The leading space is what does it: zsh honours it under `HIST_IGNORE_SPACE`
/// and bash under `HISTCONTROL=ignorespace`. Abyss syncs the pane's directory
/// on every navigation, so without this the user's history fills with `cd`.
///
/// Note there is deliberately no `z` interception here. With OSC 7 syncing in
/// place, `z foo` typed at the prompt runs the user's real zoxide, moves the
/// shell, and the pane follows on the next prompt — so intercepting it would
/// only reimplement what already works.
pub(crate) fn cd_command(directory: &Path) -> Option<String> {
    let path = directory.to_str()?;
    Some(format!(" cd {}\n", shell_words::quote(path)))
}
