use super::{Monitor, TOP_PROCESSES, ratio};

#[test]
fn ratio_is_a_fraction_and_survives_a_zero_total() {
    assert_eq!(ratio(0, 0), 0.0, "an empty device must not divide by zero");
    assert_eq!(ratio(50, 100), 0.5);
    assert_eq!(ratio(100, 100), 1.0);
    // Reported usage can exceed capacity on some filesystems; clamp rather
    // than hand a gauge a value above 1.
    assert_eq!(ratio(150, 100), 1.0);
}

#[test]
fn a_new_monitor_has_already_sampled_the_machine() {
    let monitor = Monitor::new();
    assert!(
        monitor.memory_total > 0,
        "every machine reports some total memory"
    );
    assert!(
        !monitor.cpu_per_core.is_empty(),
        "every machine reports at least one core"
    );
}

#[test]
fn the_process_table_is_capped_and_ordered_by_load() {
    let monitor = Monitor::new();
    assert!(
        monitor.processes.len() <= TOP_PROCESSES,
        "the table must stay readable"
    );
    for pair in monitor.processes.windows(2) {
        assert!(
            pair[0].cpu >= pair[1].cpu || pair[0].memory >= pair[1].memory,
            "processes should be ordered busiest first"
        );
    }
}

#[test]
fn resampling_is_rate_limited() {
    let mut monitor = Monitor::new();
    // new() has just sampled, so an immediate tick is a no-op.
    assert!(
        !monitor.tick(),
        "sampling again straight away wastes power for no new data"
    );
}

#[test]
fn memory_used_never_exceeds_the_total() {
    let monitor = Monitor::new();
    assert!(monitor.memory_used <= monitor.memory_total);
    assert!(monitor.swap_used <= monitor.swap_total);
}
