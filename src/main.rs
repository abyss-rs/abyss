use abyss::app;
use abyss::cleaner;
use abyss::events;
use abyss::ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use app::App;
use events::handle_events;
use ui::{render_delete_confirm, render_help_bar, render_progress_bar, render_status_bar};

/// Abyss - Dual-pane file manager with K8s/Cloud support + Disk Cleaner
#[derive(Parser)]
#[command(name = "abyss")]
#[command(author, version, about, long_about = None)]
#[command(after_help = r#"CLEANER COMMANDS:
  abyss clean [PATH]         Scan and clean temp files (node_modules, target, .terraform, etc.)
  abyss clean -d             Dry run - preview without deleting
  abyss clean -i             Interactive ncdu-like TUI mode
  abyss clean --days 7       Only delete items older than 7 days

For more cleaner options: abyss clean --help
"#)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Clean development temp files (node_modules, target, .terraform, __pycache__, etc.)
    #[command(after_help = r#"EXAMPLES:
  abyss clean ~/Projects           # Scan and clean ~/Projects
  abyss clean -d                   # Dry run on home directory
  abyss clean ~/Code -d -v         # Verbose dry run
  abyss clean --days 30            # Only delete items older than 30 days
  abyss clean -i                   # Interactive TUI mode

ENVIRONMENT VARIABLES:
  CLEANER_DIRS    Comma-separated list of directory patterns
  CLEANER_FILES   Comma-separated list of file patterns
  CLEANER_DAYS    Default age filter in days

CONFIG FILE:
  Create a cleaner.toml file with [patterns] section to customize targets.
"#)]
    Clean {
        /// Target folder to scan (defaults to home directory)
        #[arg(index = 1)]
        path: Option<PathBuf>,

        /// Dry run - show what would be deleted without actually deleting
        #[arg(short = 'd', long = "dry-run", default_value = "false")]
        dry_run: bool,

        /// Verbose output - show all matched paths
        #[arg(short = 'v', long = "verbose", default_value = "false")]
        verbose: bool,

        /// Number of threads for scanning and deletion (default: CPU cores)
        #[arg(short = 'j', long = "threads")]
        threads: Option<usize>,

        /// Only delete items older than N days
        #[arg(long = "days")]
        days: Option<u64>,

        /// Interactive TUI mode (ncdu-like)
        #[arg(short = 'i', long = "interactive")]
        interactive: bool,

        /// Path to TOML config file
        #[arg(short = 'c', long = "config")]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Clean {
            path,
            dry_run,
            verbose,
            threads,
            days,
            interactive,
            config,
        }) => {
            run_cleaner(path, dry_run, verbose, threads, days, interactive, config)?;
        }
        None => {
            // No subcommand - run normal TUI
            run_tui().await?;
        }
    }
    Ok(())
}

/// Run the cleaner (CLI or interactive TUI mode)
fn run_cleaner(
    path: Option<PathBuf>,
    dry_run: bool,
    verbose: bool,
    threads: Option<usize>,
    days: Option<u64>,
    interactive: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    // Resolve folder: positional > home directory
    let folder = path.unwrap_or_else(|| {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    });

    // Validate folder exists
    if !folder.exists() {
        eprintln!(
            "{} Folder does not exist: {}",
            "Error:".red().bold(),
            folder.display()
        );
        std::process::exit(1);
    }

    if !folder.is_dir() {
        eprintln!(
            "{} Path is not a directory: {}",
            "Error:".red().bold(),
            folder.display()
        );
        std::process::exit(1);
    }

    // Get absolute path
    let folder = folder.canonicalize().unwrap_or(folder);

    // Load configuration
    let mut config = cleaner::Config::load(config_path.as_deref());

    // CLI args override config
    if let Some(d) = days {
        config.days = Some(d);
    }

    let config = Arc::new(config);

    // Interactive TUI mode
    if interactive {
        run_cleaner_tui(folder, config)?;
        return Ok(());
    }

    // CLI mode - run scan and delete
    run_cleaner_cli(folder, config, dry_run, verbose, threads)
}

/// Run cleaner in CLI mode (non-interactive)
fn run_cleaner_cli(
    folder: PathBuf,
    config: Arc<cleaner::Config>,
    dry_run: bool,
    verbose: bool,
    threads: Option<usize>,
) -> Result<()> {
    let num_threads = threads.unwrap_or_else(num_cpus::get);

    // Print header
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════════╗"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        "║                    ABYSS CLEANER v0.1.0                      ║"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════════╝"
            .bright_cyan()
            .bold()
    );
    println!();

    if dry_run {
        println!(
            "  {} {}",
            "Mode:".bright_yellow().bold(),
            "DRY RUN (no files will be deleted)".yellow()
        );
    } else {
        println!(
            "  {} {}",
            "Mode:".bright_red().bold(),
            "LIVE (files will be permanently deleted!)".red()
        );
    }

    println!(
        "  {} {}",
        "Target:".bright_white().bold(),
        folder.display()
    );

    println!(
        "  {} {}",
        "Threads:".bright_white().bold(),
        num_threads
    );

    if let Some(days) = config.days {
        println!(
            "  {} {} days (items modified within this time are safe)",
            "Filter:".bright_white().bold(),
            days
        );
    }

    println!();

    // Show patterns being matched
    println!("  {} ", "Patterns:".bright_white().bold());
    println!(
        "    {} {}",
        "Directories:".dimmed(),
        config.directories.join(", ").dimmed()
    );
    println!(
        "    {} {}",
        "Files:".dimmed(),
        config.files.join(", ").dimmed()
    );
    println!();

    // Create shared stats
    let stats = Arc::new(cleaner::Stats::new());

    // Create channel for scan results
    let (tx, rx) = crossbeam_channel::unbounded();

    // Start timer
    let start = Instant::now();

    // Create progress bar
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message("Scanning directories...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Start scanner in separate thread
    let scanner = cleaner::Scanner::new(folder.clone(), num_threads, Arc::clone(&config));
    let scan_handle = thread::spawn(move || scanner.scan(tx));

    // Create deleter
    let deleter = cleaner::Deleter::new(Arc::clone(&stats), dry_run, verbose);

    // Process deletions (this blocks until scanner finishes and channel closes)
    deleter.process(rx);

    // Wait for scanner to complete
    let scanned_count = scan_handle.join().unwrap();

    // Stop progress bar
    pb.finish_and_clear();

    let elapsed = start.elapsed();

    // Print results
    println!();
    println!(
        "{}",
        "═══════════════════════════════════════════════════════════════"
            .bright_cyan()
    );
    println!("  {}", "Results:".bright_green().bold());
    println!();

    if dry_run {
        println!(
            "    {} {} directories",
            "Would delete:".yellow(),
            stats.directories()
        );
        println!(
            "    {} {} files",
            "Would delete:".yellow(),
            stats.files()
        );
        println!(
            "    {} {}",
            "Would free:".yellow(),
            humansize::format_size(stats.bytes(), humansize::BINARY)
        );
    } else {
        println!(
            "    {} {} directories",
            "Deleted:".green(),
            stats.directories()
        );
        println!("    {} {} files", "Deleted:".green(), stats.files());
        println!(
            "    {} {}",
            "Freed:".green(),
            humansize::format_size(stats.bytes(), humansize::BINARY)
        );
    }

    if stats.error_count() > 0 {
        println!(
            "    {} {} (permission denied or in use)",
            "Errors:".red(),
            stats.error_count()
        );
    }

    println!();
    println!(
        "    {} {} entries in {:.2?}",
        "Scanned:".dimmed(),
        scanned_count,
        elapsed
    );
    println!(
        "{}",
        "═══════════════════════════════════════════════════════════════"
            .bright_cyan()
    );
    println!();

    Ok(())
}

/// Run cleaner in interactive TUI mode
fn run_cleaner_tui(root: PathBuf, config: Arc<cleaner::Config>) -> Result<()> {
    use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
    use ratatui::prelude::*;
    use ratatui::widgets::{Block, Borders, Paragraph};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    // Cleanup terminal on panic or exit
    fn cleanup_terminal() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        let _ = io::Write::flush(&mut io::stdout());
    }

    // Set panic hook to cleanup terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        cleanup_terminal();
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create matcher
    let matcher = Arc::new(cleaner::PatternMatcher::new(Arc::clone(&config)));

    // Create progress tracker with cancel flag
    let progress = Arc::new(cleaner::ScanProgress::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let progress_clone = Arc::clone(&progress);
    let cancelled_clone = Arc::clone(&cancelled);

    // Start scan in background thread
    let root_clone = root.clone();
    let matcher_clone = Arc::clone(&matcher);
    let scan_handle = thread::spawn(move || {
        cleaner::DirTree::build_with_progress(&root_clone, &matcher_clone, progress_clone, cancelled_clone)
    });

    // Show live progress while scanning with quit support
    let mut user_quit = false;
    while !progress.is_done() && !user_quit {
        terminal.draw(|f| {
            let area = f.area();
            let files = progress.get_files();
            let dirs = progress.get_dirs();
            let bytes = progress.get_bytes();
            let size_str = humansize::format_size(bytes, humansize::BINARY);
            let phase = progress.get_phase();

            let text = format!(
                "\n\n  {} {}...\n\n  📁 {} folders\n  📄 {} files\n  💾 {}\n\n  Press 'q' to cancel",
                if phase == 0 { "⏳ Scanning" } else { "🔄 Building tree from" },
                root.display(),
                dirs,
                files,
                size_str
            );

            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Abyss Cleaner - Scanning ");
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
        })?;

        // Non-blocking key check for quit
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                        user_quit = true;
                        cancelled.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    // Cleanup if user quit during scan
    if user_quit {
        cleanup_terminal();
        println!("Scan cancelled.");
        return Ok(());
    }

    // Get the completed tree
    let dir_tree = match scan_handle.join() {
        Ok(tree) => tree,
        Err(_) => {
            cleanup_terminal();
            eprintln!("Scan thread panicked");
            return Ok(());
        }
    };

    // Create cleaner TUI app state
    let mut cleaner_app = CleanerTuiApp::new(root, matcher, dir_tree, config);

    // Main loop
    let result = run_cleaner_tui_app(&mut terminal, &mut cleaner_app);

    // Restore terminal
    cleanup_terminal();

    result
}

/// Cleaner TUI application state
struct CleanerTuiApp {
    root: PathBuf,
    current_path: PathBuf,
    path_stack: Vec<PathBuf>,
    entries: Vec<cleaner::DirEntry>,
    selected: usize,
    scroll_offset: usize,
    sort_mode: CleanerSortMode,
    confirm_delete: bool,
    confirm_clean: bool,
    status_message: Option<String>,
    status_time: Option<Instant>,
    total_size: u64,
    matcher: Arc<cleaner::PatternMatcher>,
    tree: Option<cleaner::DirTree>,
    config: Arc<cleaner::Config>,
    /// Last entered folder name (for cursor restoration on go_back)
    last_entered_folder: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CleanerSortMode {
    Size,
    Name,
}

impl CleanerTuiApp {
    fn new(
        root: PathBuf,
        matcher: Arc<cleaner::PatternMatcher>,
        tree: cleaner::DirTree,
        config: Arc<cleaner::Config>,
    ) -> Self {
        let mut app = Self {
            current_path: root.clone(),
            root,
            path_stack: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            sort_mode: CleanerSortMode::Size,
            confirm_delete: false,
            confirm_clean: false,
            status_message: None,
            status_time: None,
            total_size: 0,
            matcher,
            tree: Some(tree),
            config,
            last_entered_folder: None,
        };
        app.load_current_dir();
        app
    }

    fn load_current_dir(&mut self) {
        self.load_current_dir_with_selection(None);
    }

    fn load_current_dir_with_selection(&mut self, select_name: Option<&str>) {
        if let Some(ref tree) = self.tree {
            self.entries = tree.get_children(&self.current_path);
            self.apply_sort();
            self.total_size = self.entries.iter().map(|e| e.size).sum();
        }

        // Try to find and select the previously entered folder
        if let Some(name) = select_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.selected = idx;
            } else {
                self.selected = 0;
            }
        } else {
            self.selected = 0;
        }

        self.scroll_offset = 0;
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn apply_sort(&mut self) {
        match self.sort_mode {
            CleanerSortMode::Size => cleaner::tree::sort_by_size(&mut self.entries),
            CleanerSortMode::Name => cleaner::tree::sort_by_name(&mut self.entries),
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn move_down(&mut self) {
        if self.selected < self.entries.len().saturating_sub(1) {
            self.selected += 1;
        }
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn go_top(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn go_bottom(&mut self) {
        self.selected = self.entries.len().saturating_sub(1);
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn enter(&mut self) {
        if let Some(entry) = self.entries.get(self.selected).cloned() {
            if entry.is_dir {
                if entry.name == ".." {
                    self.go_back();
                } else {
                    self.last_entered_folder = Some(entry.name.clone());
                    self.path_stack.push(self.current_path.clone());
                    self.current_path = entry.path.clone();
                    self.load_current_dir();
                }
            }
        }
    }

    fn go_back(&mut self) {
        if let Some(prev) = self.path_stack.pop() {
            let current_name = self.current_path.file_name()
                .map(|n| n.to_string_lossy().to_string());

            self.current_path = prev;
            self.load_current_dir_with_selection(current_name.as_deref());
        }
        self.confirm_delete = false;
        self.confirm_clean = false;
    }

    fn toggle_sort(&mut self) {
        self.sort_mode = match self.sort_mode {
            CleanerSortMode::Size => CleanerSortMode::Name,
            CleanerSortMode::Name => CleanerSortMode::Size,
        };
        self.apply_sort();
    }

    fn toggle_delete_confirm(&mut self) {
        if !self.entries.is_empty() {
            let entry = &self.entries[self.selected];
            if entry.name != ".." {
                self.confirm_delete = !self.confirm_delete;
                self.confirm_clean = false;
            }
        }
    }

    fn toggle_clean_confirm(&mut self) {
        self.confirm_clean = !self.confirm_clean;
        self.confirm_delete = false;
    }

    fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_time = Some(Instant::now());
    }

    fn tick(&mut self) {
        // Clear expired status message
        if let Some(time) = self.status_time {
            if time.elapsed().as_secs() >= 10 {
                self.status_message = None;
                self.status_time = None;
            }
        }
    }

    fn delete_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected).cloned() {
            if entry.name == ".." {
                self.confirm_delete = false;
                return;
            }

            let result = if entry.is_dir {
                std::fs::remove_dir_all(&entry.path)
            } else {
                std::fs::remove_file(&entry.path)
            };

            match result {
                Ok(_) => {
                    self.set_status(format!(
                        "Deleted: {} ({})",
                        entry.name,
                        humansize::format_size(entry.size, humansize::BINARY)
                    ));

                    // Update tree in-memory
                    if let Some(ref mut tree) = self.tree {
                        tree.delete_entry(&entry.path, entry.is_dir);
                    }

                    // Reload and keep cursor near deleted item
                    self.load_current_dir_with_selection(Some(&entry.name));
                }
                Err(e) => {
                    self.set_status(format!("Error: {}", e));
                }
            }
        }
        self.confirm_delete = false;
    }

    fn clean_current(&mut self) {
        let root = self.current_path.clone();
        let config = Arc::clone(&self.config);

        let stats = Arc::new(cleaner::Stats::new());
        let (tx, rx) = crossbeam_channel::unbounded();
        let scanner = cleaner::Scanner::new(root, num_cpus::get(), config);

        // Run scanner
        let _scanned = scanner.scan(tx);

        // Process deletions
        let deleter = cleaner::Deleter::new(Arc::clone(&stats), false, false);
        deleter.process(rx);

        self.set_status(format!(
            "Cleaned: {} dirs, {} files ({})",
            stats.directories(),
            stats.files(),
            humansize::format_size(stats.bytes(), humansize::BINARY)
        ));

        // Rebuild tree
        self.rebuild_tree();
        self.confirm_clean = false;
    }

    fn rebuild_tree(&mut self) {
        use std::sync::atomic::AtomicBool;
        let progress = Arc::new(cleaner::ScanProgress::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        self.tree = Some(cleaner::DirTree::build_with_progress(
            &self.root,
            &self.matcher,
            progress,
            cancelled,
        ));
        self.load_current_dir();
    }

    fn refresh(&mut self) {
        self.rebuild_tree();
        self.set_status("Refreshed".to_string());
    }

    fn selected_entry(&self) -> Option<&cleaner::DirEntry> {
        self.entries.get(self.selected)
    }
}

/// Run cleaner TUI main loop
fn run_cleaner_tui_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut CleanerTuiApp,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use ratatui::prelude::*;
    use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
    use std::time::Duration;

    const TEMP_COLOR: Color = Color::Red;
    const DIR_COLOR: Color = Color::Blue;
    const FILE_COLOR: Color = Color::White;

    loop {
        app.tick();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Min(5),    // List
                    Constraint::Length(3), // Footer
                ])
                .split(f.area());

            // Header
            let path_str = app.current_path.to_string_lossy();
            let total_size = humansize::format_size(app.total_size, humansize::BINARY);
            let sort_str = match app.sort_mode {
                CleanerSortMode::Size => "size",
                CleanerSortMode::Name => "name",
            };

            let header = Paragraph::new(format!(
                " {} │ Total: {} │ Sort: {} │ {} items",
                path_str,
                total_size,
                sort_str,
                app.entries.len()
            ))
            .block(Block::default().borders(Borders::ALL).title(" Abyss Cleaner "));

            f.render_widget(header, chunks[0]);

            // List
            let items: Vec<ListItem> = app
                .entries
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let size_str = humansize::format_size(entry.size, humansize::BINARY);
                    let prefix = if entry.is_dir { "▸ " } else { "  " };
                    let temp_marker = if entry.is_temp { " [TEMP]" } else { "" };

                    let text = format!(
                        "{}{:<40} {:>10}{}",
                        prefix, entry.name, size_str, temp_marker
                    );

                    let style = if i == app.selected {
                        Style::default().bg(Color::DarkGray).bold()
                    } else if entry.is_temp {
                        Style::default().fg(TEMP_COLOR)
                    } else if entry.is_dir {
                        Style::default().fg(DIR_COLOR)
                    } else {
                        Style::default().fg(FILE_COLOR)
                    };

                    ListItem::new(text).style(style)
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::DarkGray));

            let mut state = ListState::default();
            state.select(Some(app.selected));

            f.render_stateful_widget(list, chunks[1], &mut state);

            // Footer
            let text = if app.confirm_clean {
                format!(
                    " Clean all temp files in '{}'? (y/n)",
                    app.current_path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| app.current_path.to_string_lossy().to_string())
                )
            } else if app.confirm_delete {
                if let Some(entry) = app.selected_entry() {
                    format!(
                        " Delete '{}'? (y/n) - {} will be freed",
                        entry.name,
                        humansize::format_size(entry.size, humansize::BINARY)
                    )
                } else {
                    " Delete? (y/n)".to_string()
                }
            } else if let Some(ref msg) = app.status_message {
                format!(" {} │ c:clean  d:delete  s:sort  r:refresh  q:quit", msg)
            } else {
                " ↑↓:nav  Enter:open  ←:back  c:clean  d:delete  s:sort  r:refresh  q:quit".to_string()
            };

            let style = if app.confirm_delete || app.confirm_clean {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            };

            let footer = Paragraph::new(text)
                .style(style)
                .block(Block::default().borders(Borders::ALL));

            f.render_widget(footer, chunks[2]);
        })?;

        // Non-blocking poll
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => app.enter(),
                        KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => app.go_back(),
                        KeyCode::Char('c') => app.toggle_clean_confirm(),
                        KeyCode::Char('d') => app.toggle_delete_confirm(),
                        KeyCode::Char('y') if app.confirm_delete => app.delete_selected(),
                        KeyCode::Char('y') if app.confirm_clean => app.clean_current(),
                        KeyCode::Char('n') if app.confirm_delete => app.confirm_delete = false,
                        KeyCode::Char('n') if app.confirm_clean => app.confirm_clean = false,
                        KeyCode::Char('s') => app.toggle_sort(),
                        KeyCode::Char('r') => app.refresh(),
                        KeyCode::Home | KeyCode::Char('g') => app.go_top(),
                        KeyCode::End | KeyCode::Char('G') => app.go_bottom(),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Run normal dual-pane TUI
async fn run_tui() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new().await?;

    // Main loop
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            // Determine if we need progress bar
            let show_progress = app.progress.is_some();

            let chunks = if show_progress {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),    // Main area
                        Constraint::Length(3), // Progress bar
                        Constraint::Length(1), // Status bar
                        Constraint::Length(1), // Help bar
                    ])
                    .split(f.area())
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),    // Main area
                        Constraint::Length(1), // Status bar
                        Constraint::Length(1), // Help bar
                    ])
                    .split(f.area())
            };

            // Check if in DiskAnalyzer mode for single-pane layout
            if matches!(app.mode, app::AppMode::DiskAnalyzer) {
                // Single pane for disk analyzer - render via components
                ui::components::render_disk_analyzer(f, app, chunks[0]);
            } else if !matches!(app.mode, app::AppMode::EditFile | app::AppMode::EditorSearch) {
                // Normal 2-pane layout
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(chunks[0]);

                app.left_pane.render(f, panes[0]);
                app.right_pane.render(f, panes[1]);
            }

            // Render delete confirmation popup if in ConfirmDelete mode
            if matches!(app.mode, app::AppMode::ConfirmDelete) {
                if let Some(ref target) = app.delete_target {
                    render_delete_confirm(f, target);
                }
            }

            // Render rename popup
            if matches!(app.mode, app::AppMode::Rename) {
                ui::components::render_rename_popup(f, &app.text_input);
            }

            // Render search popup
            if matches!(app.mode, app::AppMode::Search) {
                ui::components::render_search_popup(f, &app.text_input, " Search ");
            }

            // Render file editor
            if matches!(app.mode, app::AppMode::EditFile | app::AppMode::EditorSearch) {
                ui::components::render_file_editor(f, &mut app.editor, chunks[0]);
            }

            if matches!(app.mode, app::AppMode::EditorSearch) {
                ui::components::render_search_popup(f, &app.text_input, " Where Is ");
            }

            // Render large file confirmation
            if matches!(app.mode, app::AppMode::ConfirmLargeLoad) {
                ui::components::render_confirm_large_load_popup(f, app);
            }

            if show_progress {
                if let Some(ref progress) = app.progress {
                    render_progress_bar(f, chunks[1], progress);
                }
                render_status_bar(f, chunks[2], app);
                render_help_bar(f, chunks[3], app);
            } else {
                render_status_bar(f, chunks[1], app);
                render_help_bar(f, chunks[2], app);
            }
        })?;

        // Clear expired messages (after 7 seconds)
        app.clear_expired_message();

        // Poll background tasks for progress updates
        app.poll_background_task().await;

        handle_events(app).await?;

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
