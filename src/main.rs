#[cfg(not(windows))]
compile_error!("winforge targets Windows only");

mod action;
mod catalog;
mod engine;
mod mas;
mod schema;
mod select;
mod sys;
mod tui;
mod winget;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use crate::action::Status;
use crate::schema::{Pack, Preset};
use crate::select::{Selection, Tier};

const MIN_BUILD: u32 = 17763;

const USAGE: &str = "\
windows tweak engine

usage:
  winforge                          interactive mode
  winforge --dump                   print the embedded pack catalog as JSON
  winforge --list                   list packs, tweaks and their status
  winforge --select <spec>... [--preset <tier>] (--apply | --reset) [--yes]

specs:
  <tweak-id>       act on one tweak
  <pack-id>:all    act on every tweak of one pack (oneway packs included)
  all              act on every tweak of every pack

options:
  --select <spec>...  tweaks and packs to act on; repeatable
  --preset <tier>     min | std | max; adds every matching non-oneway tweak
  --apply             apply the selection
  --reset             reset the selection to Windows defaults
  --yes               skip confirmation
  --dump              print the embedded pack catalog as JSON
  --list              list packs, tweaks and their status
  --help              show this help

oneway tweaks are one-way: applying them prints a warning and can never be undone
";

enum Mode {
    Apply,
    Reset,
}

struct Cli {
    specs: Vec<String>,
    preset: Option<Tier>,
    mode: Mode,
    yes: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help") {
        let _ = write!(
            io::stdout(),
            "winforge {}\n{USAGE}",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    if args.len() == 1 && args[0] == "--dump" {
        return cmd_dump();
    }
    if args.is_empty() {
        return run_tui();
    }
    if args.len() == 1 && args[0] == "--list" {
        if let Some(code) = gate(&args) {
            return code;
        }
        return cmd_list();
    }
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("winforge: {message}");
            let _ = write!(io::stderr(), "\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let packs = catalog::packs();
    let selection = match resolve_selection(&packs, &cli) {
        Ok(selection) => selection,
        Err(message) => {
            eprintln!("winforge: {message}");
            return ExitCode::from(2);
        }
    };
    if let Some(code) = gate(&args) {
        return code;
    }
    return execute(&packs, &selection, &cli);
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut specs: Vec<String> = Vec::new();
    let mut preset: Option<Tier> = None;
    let mut mode: Option<Mode> = None;
    let mut yes = false;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--select" => {
                if mode.is_some() {
                    return Err(String::from("--select must appear before --apply/--reset"));
                }
                index += 1;
                let mut taken = 0usize;
                while index < args.len() && !args[index].starts_with("--") {
                    specs.push(args[index].clone());
                    taken += 1;
                    index += 1;
                }
                if taken == 0 {
                    return Err(String::from("--select requires at least one spec"));
                }
            }
            "--preset" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(String::from("--preset requires min|std|max"));
                };
                preset = Some(match value.as_str() {
                    "min" => Tier::Min,
                    "std" => Tier::Std,
                    "max" => Tier::Max,
                    other => return Err(format!("unknown preset '{other}'; expected min|std|max")),
                });
                index += 1;
            }
            "--apply" | "--reset" => {
                if mode.is_some() {
                    return Err(String::from("--apply and --reset are mutually exclusive"));
                }
                mode = Some(if arg == "--apply" {
                    Mode::Apply
                } else {
                    Mode::Reset
                });
                index += 1;
            }
            "--yes" => {
                yes = true;
                index += 1;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let Some(mode) = mode else {
        return Err(String::from("nothing to do; pass --apply or --reset"));
    };
    return Ok(Cli {
        specs,
        preset,
        mode,
        yes,
    });
}

fn resolve_selection(packs: &[Pack], cli: &Cli) -> Result<Selection, String> {
    let mut selection = Selection::new();
    for spec in &cli.specs {
        if spec == "all" {
            selection.select_all(packs, true);
            continue;
        }
        if let Some(category) = spec.strip_suffix(":all") {
            if packs.iter().any(|pack| pack.meta.id == category) {
                selection.select_category(packs, category, true);
                continue;
            }
            return Err(format!("unknown tweak or pack '{spec}'"));
        }
        if packs
            .iter()
            .any(|pack| pack.tweak.iter().any(|tweak| &tweak.id == spec))
        {
            selection.select_tweak(spec, true);
            continue;
        }
        return Err(format!("unknown tweak or pack '{spec}'"));
    }
    if let Some(tier) = cli.preset {
        selection.apply_preset(packs, tier);
    }
    return Ok(selection);
}

fn execute(packs: &[Pack], selection: &Selection, cli: &Cli) -> ExitCode {
    sys::enable_vt();
    if selection.is_empty() {
        println!("winforge: nothing selected");
        return ExitCode::SUCCESS;
    }
    let ids = selection.ids();
    let apply = match cli.mode {
        Mode::Apply => true,
        Mode::Reset => false,
    };
    let oneway = selection_hits_oneway(packs, selection);
    if cli.yes {
        if apply && oneway {
            println!(
                "{}",
                err("WARNING: the selection includes one-way tweaks; this CANNOT be undone")
            );
        }
    } else {
        let question = match (apply, oneway) {
            (true, true) => format!("apply {} tweaks? this cannot be undone", ids.len()),
            (true, false) => format!("apply {} tweaks?", ids.len()),
            (false, _) => format!("reset {} tweaks to default?", ids.len()),
        };
        if !confirm(&question) {
            println!("winforge: aborted");
            return ExitCode::SUCCESS;
        }
    }
    let mut total_failed = 0usize;
    for pack in packs {
        let pack_ids: Vec<String> = pack
            .tweak
            .iter()
            .filter(|tweak| ids.contains(&tweak.id))
            .map(|tweak| tweak.id.clone())
            .collect();
        if pack_ids.is_empty() {
            continue;
        }
        let report = if apply {
            engine::apply_tweaks(pack, &pack_ids)
        } else {
            engine::reset_tweaks(pack, &pack_ids)
        };
        total_failed += report.failed;
        print_report(&format!("winforge: {}", pack.meta.id), &report);
    }
    run_refresh();
    return if total_failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
}

fn selection_hits_oneway(packs: &[Pack], selection: &Selection) -> bool {
    for pack in packs {
        if !pack.meta.oneway() {
            continue;
        }
        if pack.tweak.iter().any(|tweak| selection.contains(&tweak.id)) {
            return true;
        }
    }
    return false;
}

fn cmd_list() -> ExitCode {
    sys::enable_vt();
    let packs = catalog::packs();
    if packs.is_empty() {
        eprintln!("winforge: no packs embedded");
        return ExitCode::FAILURE;
    }
    for pack in &packs {
        let oneway = pack.meta.oneway();
        if oneway {
            println!("# {}  {}  [oneway]", pack.meta.id, pack.meta.title);
        } else {
            println!("# {}  {}", pack.meta.id, pack.meta.title);
        }
        for (tweak, status) in engine::tweak_statuses(pack) {
            let letter = match status {
                Status::Applied => 'A',
                Status::Default => 'D',
                Status::Mixed => 'M',
                Status::Unknown => 'U',
                Status::Absent => 'X',
            };
            let preset = if oneway {
                String::from("-")
            } else {
                match tweak.preset {
                    Some(Preset::Min) => String::from("min"),
                    Some(Preset::Std) => String::from("std"),
                    Some(Preset::Max) => String::from("max"),
                    Some(Preset::Manual) => String::from("manual"),
                    None => String::from("-"),
                }
            };
            println!("  [{letter}] {preset}  {}  {}", tweak.id, tweak.name);
        }
    }
    return ExitCode::SUCCESS;
}

fn run_tui() -> ExitCode {
    if let Some(code) = gate(&[]) {
        return code;
    }
    tui::run();
    sys::log("winforge exit");
    return ExitCode::SUCCESS;
}

fn gate(args: &[String]) -> Option<ExitCode> {
    sys::log("winforge start");
    let build = match sys::windows_build() {
        Ok(build) => build,
        Err(message) => {
            eprintln!("winforge: {message}");
            maybe_wait();
            return Some(ExitCode::FAILURE);
        }
    };
    if build < MIN_BUILD {
        println!(
            "winforge: Windows build {build} is too old; build {MIN_BUILD} (1809) or newer is required"
        );
        maybe_wait();
        return Some(ExitCode::from(1));
    }
    if !sys::is_admin() {
        println!("winforge: requesting administrator rights...");
        match sys::elevate(args) {
            Ok(()) => {
                sys::log("relaunched elevated");
                return Some(ExitCode::SUCCESS);
            }
            Err(message) => {
                println!("winforge: {message}");
                sys::log(&format!("elevation failed: {message}"));
                maybe_wait();
                return Some(ExitCode::FAILURE);
            }
        }
    }
    return None;
}

fn maybe_wait() {
    if sys::stdin_is_console() {
        wait_enter();
    }
}

fn cmd_dump() -> ExitCode {
    let packs = catalog::packs();
    if packs.is_empty() {
        eprintln!("winforge: no packs embedded");
        return ExitCode::FAILURE;
    }
    match serde_json::to_string_pretty(&packs) {
        Ok(text) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("winforge: dump serialization failed: {err}");
            return ExitCode::FAILURE;
        }
    }
}

fn run_refresh() {
    let Some(script) = catalog::script("refresh.ps1") else {
        return;
    };
    println!("{}", dim("running refresh..."));
    match sys::run_script(script, sys::TIMEOUT_SCRIPT) {
        Ok(out) if out.status.success() => {
            sys::log("refresh.ps1 done");
        }
        Ok(out) => {
            sys::log(&format!(
                "refresh.ps1 exit {}",
                out.status.code().unwrap_or(-1)
            ));
            println!(
                "{}",
                warn(&format!(
                    "winforge: refresh exited {}",
                    out.status.code().unwrap_or(-1)
                ))
            );
        }
        Err(message) => {
            sys::log(&format!("refresh.ps1 failed: {message}"));
            println!("{}", warn(&format!("winforge: refresh failed: {message}")));
        }
    }
}

fn print_report(label: &str, report: &engine::Report) {
    let line = format!(
        "{label}: {} done, {} skipped, {} failed",
        report.done, report.skipped, report.failed
    );
    if report.ok() {
        println!("{}", ok(&line));
    } else {
        println!("{}", err(&line));
    }
    for failure in &report.failures {
        println!("  {}", warn(&format!("failed: {failure}")));
    }
}

fn ok(text: &str) -> String {
    return colored("32", text);
}

fn warn(text: &str) -> String {
    return colored("33", text);
}

fn err(text: &str) -> String {
    return colored("31", text);
}

fn dim(text: &str) -> String {
    return colored("90", text);
}

fn colored(code: &str, text: &str) -> String {
    return format!("\x1b[{code}m{text}\x1b[0m");
}

fn prompt(text: &str) -> Option<String> {
    print!("{text} ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    return match io::stdin().lock().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim().to_string()),
        Err(_) => None,
    };
}

fn confirm(text: &str) -> bool {
    let Some(answer) = prompt(&format!("{text} [y/N]")) else {
        return false;
    };
    return answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes");
}

fn wait_enter() {
    print!("press ENTER to exit... ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
}
