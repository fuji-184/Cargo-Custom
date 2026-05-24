use std::env;
use std::process::{Command, exit};

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  cargo custom [check|run|build] [options]");
    eprintln!("  cargo custom miri [check|run] [options]");
    eprintln!("  cargo custom cranelift [check|run|build] [options]");
    eprintln!("  cargo custom -h | --help");
}

fn run_clear() {
    let clear_status = Command::new("clear").status();
    if clear_status.is_err() {
        exit(1);
    }
}

fn get_base_rust_flags(use_cranelift: bool) -> String {
    let mut flags = String::from(
        "
        -Zthreads=0
        -Zshare-generics=y
        -C debuginfo=0
        -C prefer-dynamic
        -C link-arg=-Wl,--threads=0
        -C metadata=dev
        -Zinline-mir=off
        -Zproc-macro-backtrace=off
        -Zvalidate-mir=off
        -C embed-bitcode=no
        
        -Zmir-opt-level=0
        -Zmerge-functions=disabled
        ",
    );
    if use_cranelift {
        flags.push_str(
            "
            -Zcodegen-backend=cranelift
            ",
        );
    } else {
        flags.push_str(
            "
            -C llvm-args=--inline-threshold=0
            -C no-prepopulate-passes
            ",
        );
    }
    let mut clean_flags = flags.replace("\n", " ");
    let mut mold_available = false;
    if let Ok(status) = Command::new("mold").arg("--version").status() {
        if status.success() {
            mold_available = true;
        }
    }

    if mold_available {
        clean_flags.push_str(" -C link-arg=-fuse-ld=mold -C link-arg=-Wl,--threads=0");
    } else {
        if let Ok(status) = Command::new("lld").arg("--version").status() {
            if status.success() {
                clean_flags.push_str(" -C link-arg=-fuse-ld=lld -C link-arg=-Wl,--threads=0");
            }
        }
    }
    clean_flags
}

fn set_sccache_if_available(cmd: &mut Command) {
    if let Ok(status) = Command::new("sccache").arg("--version").status() {
        if status.success() {
            cmd.env("RUSTC_WRAPPER", "sccache");
        }
    }
}

fn handle_standard_action(action: &str, remaining_args: &[&str]) {
    run_clear();
    let mut cmd = Command::new("cargo");
    cmd.arg(action);
    let rust_flags = get_base_rust_flags(false);
    cmd.env("RUSTFLAGS", rust_flags);
    set_sccache_if_available(&mut cmd);
    cmd.args(remaining_args);
    let next_status = cmd.status();
    match next_status {
        Ok(status) => {
            if status.success() {
                exit(0);
            } else {
                exit(status.code().unwrap_or(1));
            }
        }
        Err(_) => exit(1),
    }
}

fn handle_miri_action(miri_action: &str, remaining_args: &[&str]) {
    run_clear();
    let mut cmd = Command::new("cargo");
    cmd.arg("miri").arg(miri_action);
    let miri_flags = "
-Zmiri-disable-validation 
                      -Zmiri-disable-alignment-check 
                      -Zmiri-disable-data-race-detector 
                      -Zmiri-ignore-leaks 
                      -Zmiri-disable-isolation 
                      -Zmiri-preemption-rate=0 
                      -Zmiri-provenance-gc=0 
                      -Zmiri-no-extra-rounding-error
".replace("\n", " ");
    let rust_flags = get_base_rust_flags(false);
    cmd.env("MIRIFLAGS", miri_flags);
    cmd.env("RUSTFLAGS", rust_flags);
    cmd.args(remaining_args);
    let next_status = cmd.status();
    match next_status {
        Ok(status) => {
            if status.success() {
                exit(0);
            } else {
                exit(status.code().unwrap_or(1));
            }
        }
        Err(_) => exit(1),
    }
}

fn handle_cranelift_action(cranelift_action: &str, remaining_args: &[&str]) {
    run_clear();
    let mut cmd = Command::new("cargo");
    cmd.arg(cranelift_action);
    let rust_flags = get_base_rust_flags(true);
    cmd.env("RUSTFLAGS", rust_flags);
    set_sccache_if_available(&mut cmd);
    cmd.args(remaining_args);
    let next_status = cmd.status();
    match next_status {
        Ok(status) => {
            if status.success() {
                exit(0);
            } else {
                exit(status.code().unwrap_or(1));
            }
        }
        Err(_) => exit(1),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage();
        exit(1);
    }
    let arg1 = args[2].as_str();
    if arg1 == "-h" || arg1 == "--help" {
        print_usage();
        exit(0);
    }
    if arg1 == "miri" {
        if args.len() < 4 {
            eprintln!("Missing sub-command for 'miri'. Expected 'check' or 'run'.");
            eprintln!();
            print_usage();
            exit(1);
        }
        let miri_action = args[3].as_str();
        match miri_action {
            "check" | "run" => {
                let remaining_args: Vec<&str> = args.iter().skip(4).map(|s| s.as_str()).collect();
                handle_miri_action(miri_action, &remaining_args);
            }
            _ => {
                eprintln!("Unknown sub-command for 'miri': {}", miri_action);
                eprintln!();
                print_usage();
                exit(1);
            }
        }
    } else if arg1 == "cranelift" {
        if args.len() < 4 {
            eprintln!("Missing sub-command for 'cranelift'. Expected 'check', 'run', or 'build'.");
            eprintln!();
            print_usage();
            exit(1);
        }
        let cranelift_action = args[3].as_str();
        match cranelift_action {
            "check" | "run" | "build" => {
                let remaining_args: Vec<&str> = args.iter().skip(4).map(|s| s.as_str()).collect();
                handle_cranelift_action(cranelift_action, &remaining_args);
            }
            _ => {
                eprintln!("Unknown sub-command for 'cranelift': {}", cranelift_action);
                eprintln!();
                print_usage();
                exit(1);
            }
        }
    } else {
        match arg1 {
            "check" | "run" | "build" => {
                let remaining_args: Vec<&str> = args.iter().skip(3).map(|s| s.as_str()).collect();
                handle_standard_action(arg1, &remaining_args);
            }
            _ => {
                eprintln!("Unknown command: {}", arg1);
                eprintln!();
                print_usage();
                exit(1);
            }
        }
    }
}
