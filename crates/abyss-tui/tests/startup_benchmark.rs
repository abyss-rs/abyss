use std::time::Instant;

#[test]
fn benchmark_startup_phases() {
    let overall = Instant::now();

    // Phase 1: Workspace load
    let t0 = Instant::now();
    let (_workspace, _warning) = abyss_core::workspace::WorkspaceState::load_default();
    let t_workspace = t0.elapsed();

    // Phase 2: BrowserService creation
    let t1 = Instant::now();
    let browser = abyss_core::browser::BrowserService::new();
    let t_browser = t1.elapsed();

    // Phase 3: Pane setup & reload
    let t2 = Instant::now();
    let current = std::env::current_dir()
        .map(abyss_core::storage::Location::Local)
        .unwrap_or_else(|_| abyss_core::storage::Location::Local(std::path::PathBuf::from(".")));
    let mut pane0 = abyss_core::browser::Pane::new(current.clone());
    let mut pane1 = abyss_core::browser::Pane::new(current.clone());
    pane0.reload(0, &browser);
    pane1.reload(1, &browser);
    let t_panes = t2.elapsed();

    // Phase 4: Storage runtime lazy verification (must be uninitialized)
    assert!(
        !browser.is_storage_initialized(),
        "Storage runtime must NOT be initialized during normal startup"
    );

    let total = overall.elapsed();

    // Phase 5: On-demand remote invocation check
    let t_remote_start = Instant::now();
    let _storage = browser.storage();
    let t_remote_init = t_remote_start.elapsed();
    assert!(
        browser.is_storage_initialized(),
        "Storage runtime must be initialized after explicit remote request"
    );

    println!("\n=== Startup Phase Breakdown ===");
    println!("1. Workspace load:        {:?}", t_workspace);
    println!("2. BrowserService new:    {:?}", t_browser);
    println!("3. Panes initial reload:  {:?}", t_panes);
    println!("4. Storage runtime at start: UNINITIALIZED (0 overhead)");
    println!("5. On-demand remote init: {:?}", t_remote_init);
    println!("Total headless startup:   {:?}", total);
    println!("================================\n");
}
