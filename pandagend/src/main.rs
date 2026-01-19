//! # PandaGen Host Daemon
//!
//! Main entry point for the PandaGen host runtime.

use pandagend::{HostMode, HostRuntime, HostRuntimeConfig};
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = parse_args(&args).unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        print_usage(&args[0]);
        process::exit(1);
    });

    let mut runtime = HostRuntime::new(config).unwrap_or_else(|e| {
        eprintln!("Failed to create runtime: {}", e);
        process::exit(1);
    });

    if let Err(e) = runtime.run() {
        eprintln!("Runtime error: {}", e);
        process::exit(1);
    }
}

fn parse_args(args: &[String]) -> Result<HostRuntimeConfig, String> {
    let mut config = HostRuntimeConfig::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--mode" | "-m" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --mode".to_string());
                }
                config.mode = match args[i].as_str() {
                    "sim" => HostMode::Sim,
                    #[cfg(feature = "hal_mode")]
                    "hal" => HostMode::Hal,
                    other => return Err(format!("Invalid mode: {}", other)),
                };
            }
            "--script" | "-s" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --script".to_string());
                }
                let script_path = &args[i];
                let script_text = fs::read_to_string(script_path)
                    .map_err(|e| format!("Failed to read script file: {}", e))?;
                config.script = Some(script_text);
            }
            "--max-steps" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --max-steps".to_string());
                }
                config.max_steps = args[i]
                    .parse()
                    .map_err(|_| format!("Invalid max-steps value: {}", args[i]))?;
            }
            "--exit-on-idle" => {
                config.exit_on_idle = true;
            }
            "--help" | "-h" => {
                print_usage(&args[0]);
                process::exit(0);
            }
            other => {
                return Err(format!("Unknown option: {}", other));
            }
        }
        i += 1;
    }

    Ok(config)
}

fn print_usage(program: &str) {
    eprintln!("Usage: {} [OPTIONS]", program);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -m, --mode <MODE>        Host mode: sim (default)");
    #[cfg(feature = "hal_mode")]
    eprintln!("                           or hal (NOTE: hal mode is not yet functional)");
    eprintln!("  -s, --script <FILE>      Input script file (for sim mode)");
    eprintln!("  --max-steps <N>          Maximum steps to run (0 = unlimited)");
    eprintln!("  --exit-on-idle           Exit when no components are running");
    eprintln!("  -h, --help               Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!(
        "  {} --mode sim --script examples/hello_editor.pgkeys",
        program
    );
    eprintln!("  {} --max-steps 100 --exit-on-idle", program);
}
