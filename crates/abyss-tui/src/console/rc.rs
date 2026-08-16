use std::fs;
use std::path::Path;

use tempfile::TempDir;

/// Environment tweaks that make a shell report its directory over OSC 7.
pub(crate) struct ShellHook {
    /// Extra environment for the spawned shell.
    pub(crate) env: Vec<(String, String)>,
    /// Extra arguments for the shell.
    pub(crate) args: Vec<String>,
    /// Temporary directory holding generated rc files; kept alive by the
    /// caller so the shell can still read them after we return.
    pub(crate) _scratch: Option<TempDir>,
}

impl ShellHook {
    fn none() -> Self {
        Self {
            env: Vec::new(),
            args: Vec::new(),
            _scratch: None,
        }
    }
}

/// zsh runs `precmd_functions` before drawing each prompt.
const ZSH_HOOK: &str = r#"
_abyss_osc7() { printf '\033]7;file://%s%s\a' "${HOST:-}" "$PWD" }
typeset -ag precmd_functions
precmd_functions+=(_abyss_osc7)
"#;

/// bash evaluates `PROMPT_COMMAND` before each prompt.
const BASH_HOOK: &str = r#"
_abyss_osc7() { printf '\033]7;file://%s%s\a' "${HOSTNAME:-}" "$PWD"; }
PROMPT_COMMAND="_abyss_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#;

/// Build the hook for `shell`, whose own configuration is always sourced
/// first so the user's prompt, aliases and history keep working.
///
/// Shells we have no snippet for (fish, nushell, Windows shells) get nothing:
/// fish already emits OSC 7 natively, and the rest simply fall back to
/// one-directional syncing.
pub(crate) fn shell_hook(shell: &Path) -> ShellHook {
    let name = shell
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    match name {
        "zsh" => zsh_hook().unwrap_or_else(ShellHook::none),
        "bash" => bash_hook().unwrap_or_else(ShellHook::none),
        _ => ShellHook::none(),
    }
}

/// zsh has no `--rcfile`, so point `ZDOTDIR` at a directory holding a `.zshrc`
/// that sources the real one and then appends our hook.
fn zsh_hook() -> Option<ShellHook> {
    let scratch = TempDir::new().ok()?;
    let user = std::env::var("ZDOTDIR")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("HOME").ok())?;
    let script = format!(
        "[ -f {user}/.zshrc ] && source {user}/.zshrc\n{ZSH_HOOK}",
        user = shell_words::quote(&user),
    );
    fs::write(scratch.path().join(".zshrc"), script).ok()?;
    Some(ShellHook {
        env: vec![(
            "ZDOTDIR".to_owned(),
            scratch.path().to_string_lossy().into_owned(),
        )],
        args: Vec::new(),
        _scratch: Some(scratch),
    })
}

fn bash_hook() -> Option<ShellHook> {
    let scratch = TempDir::new().ok()?;
    let home = std::env::var("HOME").ok()?;
    let script = format!(
        "[ -f {home}/.bashrc ] && source {home}/.bashrc\n{BASH_HOOK}",
        home = shell_words::quote(&home),
    );
    let path = scratch.path().join("abyss-bashrc");
    fs::write(&path, script).ok()?;
    Some(ShellHook {
        env: Vec::new(),
        args: vec![
            "--rcfile".to_owned(),
            path.to_string_lossy().into_owned(),
            "-i".to_owned(),
        ],
        _scratch: Some(scratch),
    })
}
