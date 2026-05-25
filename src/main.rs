use std::env;
use std::process::{Command, exit};

const BACKENDS: &[&str] = &["cranelift"];
const LINKERS: &[&str] = &["wild", "mold", "lld"];
const ALLOCATORS: &[&str] = &["mimalloc", "jemalloc", "tcmalloc"];
const ACTIONS: &[&str] = &["check", "run", "build"];
const MIRI_ACTIONS: &[&str] = &["check", "run"];

const BASE_FLAGS: &str = concat!(
    "-Zthreads=0 ",
    "-Zshare-generics=y ",
    "-C debuginfo=0 ",
    "-C prefer-dynamic ",
    "-C metadata=dev ",
    "-Zinline-mir=off ",
    "-Zproc-macro-backtrace=off ",
    "-Zvalidate-mir=off ",
    "-C embed-bitcode=no ",
    "-Zcache-proc-macros ",
    "-C debug-assertions=no ",
    "-Zmacro-backtrace=off ",
    "-Zspan-debug=no ",
    "-Znext-solver ",
    "-Zrelax-elf-relocations=y ",
    "-Zprint-mono-items=off ",
    "-Zalways-encode-mir=no ",
    "-Zmeta-stats=no ",
    "-Zbinary-dep-depinfo=off ",
    "-Zno-implied-bounds-compat=y ",
    "-Zlayout-seed=0 ",
    "-Zno-leak-check ",
    "-Zub-checks=off ",
    "-Zincremental-info=off ",
    "-Zflatten-format-args=yes ",
    "-Zincremental-verify-ich=no ",
    "-Zdual-proc-macros",
);

const LLVM_FLAGS: &str = "-C no-prepopulate-passes";
const CRANELIFT_FLAGS: &str = "-Zcodegen-backend=cranelift";

const MIRI_FLAGS: &str = concat!(
    "-Zmiri-disable-validation ",
    "-Zmiri-disable-alignment-check ",
    "-Zmiri-disable-data-race-detector ",
    "-Zmiri-ignore-leaks ",
    "-Zmiri-disable-isolation ",
    "-Zmiri-preemption-rate=0 ",
    "-Zmiri-provenance-gc=0 ",
    "-Zmiri-no-extra-rounding-error",
);

#[derive(Clone, Copy)]
enum Backend {
    Llvm,
    Cranelift,
}

#[derive(Clone, Copy)]
enum Allocator {
    Default,
    Mimalloc,
    Jemalloc,
    Tcmalloc,
}

impl Backend {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "cranelift" => Some(Backend::Cranelift),
            _ => None,
        }
    }

    fn flags(self) -> &'static str {
        match self {
            Backend::Llvm      => LLVM_FLAGS,
            Backend::Cranelift => CRANELIFT_FLAGS,
        }
    }
}

impl Allocator {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "mimalloc" => Some(Allocator::Mimalloc),
            "jemalloc" => Some(Allocator::Jemalloc),
            "tcmalloc" => Some(Allocator::Tcmalloc),
            _ => None,
        }
    }

    fn lib_name(self) -> Option<&'static str> {
        match self {
            Allocator::Default  => None,
            Allocator::Mimalloc => Some("libmimalloc.so"),
            Allocator::Jemalloc => Some("libjemalloc.so"),
            Allocator::Tcmalloc => Some("libtcmalloc.so"),
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  cargo custom [cranelift] [wild|mold|lld] [mimalloc|jemalloc|tcmalloc] [check|run|build] [options]");
    eprintln!("  cargo custom miri [check|run] [options]");
    eprintln!("  cargo custom -h | --help");
}

fn run_clear() {
    if Command::new("clear").status().is_err() {
        exit(1);
    }
}

fn find_lib(name: &str) -> Option<String> {
    if let Ok(output) = Command::new("ldconfig").arg("-p").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(name) {
                    if let Some(idx) = line.rfind("=>") {
                        let path = line[idx + 2..].trim().to_string();
                        if std::path::Path::new(&path).exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

fn set_allocator(cmd: &mut Command, allocator: Allocator) {
    let lib = match allocator.lib_name() {
        Some(name) => name,
        None => return,
    };
    match find_lib(lib) {
        Some(path) => { cmd.env("LD_PRELOAD", path); }
        None => eprintln!("{lib} not found, using default allocator"),
    }
}

fn set_sccache_if_available(cmd: &mut Command) {
    if Command::new("sccache").arg("--version").status().map(|s| s.success()).unwrap_or(false) {
        cmd.env("RUSTC_WRAPPER", "sccache");
    }
}

fn resolve_linker(name: &str) -> Option<&'static str> {
    let (check_bin, flag) = match name {
        "wild" => ("wild",   "-Clinker=clang -Clink-args=--ld-path=wild"),
        "mold" => ("mold",   "-C link-arg=-fuse-ld=mold"),
        "lld"  => ("ld.lld", "-C link-arg=-fuse-ld=lld"),
        other  => { eprintln!("Unknown linker: {other}"); return None; }
    };
    if Command::new(check_bin).arg("--version").status().map(|s| s.success()).unwrap_or(false) {
        Some(flag)
    } else {
        eprintln!("{name} not found");
        None
    }
}

fn build_rust_flags(backend: Backend, linker: Option<&str>) -> String {
    let mut flags = format!("{} {}", BASE_FLAGS, backend.flags());
    if let Some(name) = linker {
        if let Some(linker_flags) = resolve_linker(name) {
            flags.push(' ');
            flags.push_str(linker_flags);
        }
    }
    flags
}

fn run_cargo(cargo_args: &[&str], env: &[(&str, &str)], allocator: Allocator) {
    let mut cmd = Command::new("cargo");
    cmd.args(cargo_args);
    for &(k, v) in env {
        cmd.env(k, v);
    }
    set_sccache_if_available(&mut cmd);
    set_allocator(&mut cmd, allocator);
    match cmd.status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(_) => exit(1),
    }
}

fn handle_action(action: &str, backend: Backend, linker: Option<&str>, allocator: Allocator, extra: &[&str]) {
    run_clear();
    let rust_flags = build_rust_flags(backend, linker);
    let mut cargo_args = vec![action];
    cargo_args.extend_from_slice(extra);
    let mut env: Vec<(&str, &str)> = vec![
        ("RUSTFLAGS", &rust_flags),
        ("CARGO_PROFILE_DEV_BUILD_OVERRIDE_OPT_LEVEL", "3"),
    ];
    if matches!(backend, Backend::Cranelift) {
        env.push(("CARGO_CACHE_RUSTC_INFO", "1"));
    }
    run_cargo(&cargo_args, &env, allocator);
}

fn handle_miri(miri_action: &str, extra: &[&str]) {
    run_clear();
    let rust_flags = build_rust_flags(Backend::Llvm, None);
    let mut cargo_args = vec!["miri", miri_action];
    cargo_args.extend_from_slice(extra);
    run_cargo(
        &cargo_args,
        &[
            ("RUSTFLAGS", &rust_flags),
            ("MIRIFLAGS", MIRI_FLAGS),
        ],
        Allocator::Default,
    );
}

struct ParsedArgs<'a> {
    backend:   Backend,
    linker:    Option<&'a str>,
    allocator: Allocator,
    action:    &'a str,
    rest:      Vec<&'a str>,
}

fn parse_args(args: &[String], offset: usize) -> ParsedArgs<'_> {
    let mut idx = offset;

    let backend = match args.get(idx) {
        Some(a) if BACKENDS.contains(&a.as_str()) => {
            idx += 1;
            Backend::from_str(a).unwrap()
        }
        _ => Backend::Llvm,
    };

    let linker = match args.get(idx) {
        Some(a) if LINKERS.contains(&a.as_str()) => {
            idx += 1;
            Some(a.as_str())
        }
        _ => None,
    };

    let allocator = match args.get(idx) {
        Some(a) if ALLOCATORS.contains(&a.as_str()) => {
            idx += 1;
            Allocator::from_str(a).unwrap_or(Allocator::Default)
        }
        _ => Allocator::Default,
    };

    let action = match args.get(idx) {
        Some(a) if ACTIONS.contains(&a.as_str()) => {
            idx += 1;
            a.as_str()
        }
        Some(other) => {
            eprintln!("Expected action [check|run|build], got: {other}");
            print_usage();
            exit(1);
        }
        None => {
            eprintln!("Missing action [check|run|build]");
            print_usage();
            exit(1);
        }
    };

    let rest = args.iter().skip(idx).map(|s| s.as_str()).collect();

    ParsedArgs { backend, linker, allocator, action, rest }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 || matches!(args[2].as_str(), "-h" | "--help") {
        print_usage();
        exit(if args.len() < 3 { 1 } else { 0 });
    }

    match args[2].as_str() {
        "miri" => {
            let miri_action = args.get(3).map(|s| s.as_str()).unwrap_or_else(|| {
                eprintln!("Missing sub-command for 'miri'. Expected: {MIRI_ACTIONS:?}");
                print_usage();
                exit(1);
            });
            if !MIRI_ACTIONS.contains(&miri_action) {
                eprintln!("Unknown miri sub-command: {miri_action}");
                print_usage();
                exit(1);
            }
            let extra: Vec<&str> = args.iter().skip(4).map(|s| s.as_str()).collect();
            handle_miri(miri_action, &extra);
        }
        _ => {
            let parsed = parse_args(&args, 2);
            handle_action(parsed.action, parsed.backend, parsed.linker, parsed.allocator, &parsed.rest);
        }
    }
}