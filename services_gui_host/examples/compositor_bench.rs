//! Wall-clock timing for the compositor work scenarios (GFX-045).
//!
//! Run with `cargo run --release -p services_gui_host --example compositor_bench`.

use services_gui_host::bench::run_all;
use std::time::Instant;

fn main() {
    // Warm up allocations and caches.
    let _ = run_all();
    let iterations = 20;
    let start = Instant::now();
    let mut reports = Vec::new();
    for _ in 0..iterations {
        reports = run_all();
    }
    let per_run = start.elapsed() / iterations;
    println!(
        "{:<26} {:>12} {:>8} {:>10} {:>8}",
        "scenario", "pixels", "windows", "damage", "repaint"
    );
    for report in &reports {
        println!(
            "{:<26} {:>12} {:>8} {:>10} {:>7.1}%",
            report.name,
            report.pixels_written,
            report.windows_painted,
            report.damage_area,
            report.repaint_permille() as f64 / 10.0
        );
    }
    println!("all scenarios: {:?} per run ({iterations} runs)", per_run);
}
