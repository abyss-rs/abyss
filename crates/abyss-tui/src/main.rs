use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "A fast native two-pane file manager for local, cloud, and Kubernetes storage"
)]
struct Args {
    /// Starting local path or storage URI for the left pane
    left: Option<String>,

    /// Starting local path or storage URI for the right pane
    right: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();

    match abyss_tui::run(args.left, args.right) {
        Ok(()) => ExitCode::SUCCESS,
        Err(abyss_tui::Error::Cancelled) => {
            eprintln!("operation cancelled");
            ExitCode::from(130)
        }
        Err(error) => {
            eprintln!("abyss: {error}");
            ExitCode::FAILURE
        }
    }
}
