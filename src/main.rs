use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use nix_output_monitor::build_reports;
use nix_output_monitor::io_loop::{self, InputMode};
use nix_output_monitor::print::Config;
use nix_output_monitor::state::NomState;
use nix_output_monitor::{ansi, time};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    install_signal_handlers();

    let prog_name = program_name();
    let args: Vec<String> = env::args().skip(1).collect();

    // Nix completion protocol.
    if env::var_os("NIX_GET_COMPLETIONS").is_some() {
        return print_nix_completion(&prog_name, &args);
    }

    run_app(&prog_name, &args)
}

fn program_name() -> String {
    env::args()
        .next()
        .and_then(|arg0| {
            Path::new(&arg0)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "nom".to_string())
}

fn run_app(prog_name: &str, args: &[String]) -> ExitCode {
    match (
        prog_name,
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    ) {
        (_, ["--version"]) => {
            let _ = writeln!(io::stderr(), "nix-output-monitor {VERSION}");
            forward_exit(Command::new("nix").arg("--version").status())
        }
        ("nom-build", rest) => forward_exit(run_monitored(
            default_config(),
            "nix-build",
            &with_json(rest),
        )),
        ("nom-shell", rest) => {
            let pre = run_monitored(
                Config {
                    silent: true,
                    piping: false,
                },
                "nix-shell",
                &[
                    &with_json(rest)[..],
                    &["--run".to_string(), "exit".to_string()],
                ]
                .concat(),
            );
            if !is_success(&pre) {
                return forward_exit(pre);
            }
            forward_exit(Command::new("nix-shell").args(rest).status())
        }
        ("nom", subargs) => dispatch_nom(subargs),
        _ => {
            let _ = writeln!(io::stderr(), "{}", help_text());
            if args.iter().any(|a| a == "-h" || a == "--help") {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

fn dispatch_nom(args: &[&str]) -> ExitCode {
    match args {
        ["build", rest @ ..] => {
            let mut nix_args = vec!["build".to_string()];
            nix_args.extend(with_json(rest));
            forward_exit(run_monitored(default_config(), "nix", &nix_args))
        }
        ["shell", rest @ ..] => {
            let mut nix_args = vec!["shell".to_string()];
            nix_args.extend(with_json(&replace_command_with_exit(rest)));
            let pre = run_monitored(
                Config {
                    silent: true,
                    piping: false,
                },
                "nix",
                &nix_args,
            );
            if !is_success(&pre) {
                return forward_exit(pre);
            }
            let mut second = vec!["shell".to_string()];
            second.extend(rest.iter().map(|s| s.to_string()));
            forward_exit(Command::new("nix").args(&second).status())
        }
        ["develop", rest @ ..] => {
            let mut nix_args = vec!["develop".to_string()];
            nix_args.extend(with_json(&replace_command_with_exit(rest)));
            let pre = run_monitored(
                Config {
                    silent: true,
                    piping: false,
                },
                "nix",
                &nix_args,
            );
            if !is_success(&pre) {
                return forward_exit(pre);
            }
            let mut second = vec!["develop".to_string()];
            second.extend(rest.iter().map(|s| s.to_string()));
            forward_exit(Command::new("nix").args(&second).status())
        }
        [] => match monitor_stdin(
            Config {
                piping: true,
                ..default_config()
            },
            InputMode::OldStyle,
        ) {
            Ok(final_state) => {
                if final_state.full_summary.failed_builds.is_empty()
                    && final_state.nix_errors.is_empty()
                {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(_) => ExitCode::FAILURE,
        },
        ["--json"] => match monitor_stdin(
            Config {
                piping: true,
                ..default_config()
            },
            InputMode::Json,
        ) {
            Ok(final_state) => {
                if final_state.full_summary.failed_builds.is_empty()
                    && final_state.nix_errors.is_empty()
                {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(_) => ExitCode::FAILURE,
        },
        other => {
            let _ = writeln!(io::stderr(), "{}", help_text());
            if other.iter().any(|a| *a == "-h" || *a == "--help") {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

fn monitor_stdin(config: Config, mode: InputMode) -> io::Result<NomState> {
    let platform = detect_current_system();
    let initial = NomState::new(time::now(), platform, build_reports::load());
    io_loop::run(config, mode, io::stdin(), io::stderr(), initial)
}

fn run_monitored(
    config: Config,
    command: &str,
    args: &[String],
) -> io::Result<std::process::ExitStatus> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().expect("stderr piped");

    let platform = detect_current_system();
    let initial = NomState::new(time::now(), platform, build_reports::load());
    let _ = io_loop::run(config, InputMode::Json, stderr, io::stderr(), initial)?;

    // Forward subprocess stdout.
    if let Some(mut stdout) = child.stdout.take() {
        let _ = io::copy(&mut stdout, &mut io::stdout());
    }
    child.wait()
}

fn detect_current_system() -> Option<String> {
    let output = Command::new("nix")
        .args([
            "eval",
            "--extra-experimental-features",
            "nix-command",
            "--impure",
            "--raw",
            "--expr",
            "builtins.currentSystem",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn default_config() -> Config {
    Config {
        silent: false,
        piping: false,
    }
}

/// Strip any `--command` and `-c` argument so the user's shell command does not
/// actually run in the first (silent) eval pass.
fn replace_command_with_exit(args: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .take_while(|a| **a != "--command" && **a != "-c")
        .map(|s| s.to_string())
        .collect();
    out.push("--command".to_string());
    out.push("sh".to_string());
    out.push("-c".to_string());
    out.push("exit".to_string());
    out
}

fn with_json(args: &[impl AsRef<str>]) -> Vec<String> {
    let mut v = vec![
        "-v".to_string(),
        "--log-format".to_string(),
        "internal-json".to_string(),
    ];
    for a in args {
        v.push(a.as_ref().to_string());
    }
    v
}

fn is_success(res: &io::Result<std::process::ExitStatus>) -> bool {
    matches!(res, Ok(s) if s.success())
}

fn forward_exit(res: io::Result<std::process::ExitStatus>) -> ExitCode {
    match res {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            let code = s.code().unwrap_or(1) as u8;
            ExitCode::from(code)
        }
        Err(e) => {
            let _ = writeln!(
                io::stderr(),
                "{}: {e}",
                ansi::bold_red("nix-output-monitor")
            );
            ExitCode::FAILURE
        }
    }
}

fn install_signal_handlers() {
    // On Ctrl-C / SIGTERM, re-show the cursor before letting the default
    // handler kill us. Ignored if installation fails (e.g. running under a
    // parent that already installed its own handler).
    let _ = ctrlc::set_handler(|| {
        let mut stderr = io::stderr();
        let _ = stderr.write_all(ansi::SHOW_CURSOR.as_bytes());
        let _ = stderr.flush();
        // 128 + SIGINT(2) is the conventional exit code.
        std::process::exit(130);
    });
}

fn print_nix_completion(prog: &str, args: &[String]) -> ExitCode {
    match (prog, args) {
        ("nom", [input]) => {
            println!("normal");
            let known: [&str; 7] = [
                "build",
                "shell",
                "develop",
                "--version",
                "-h",
                "--help",
                "--json",
            ];
            for k in known.iter().filter(|k| k.starts_with(input.as_str())) {
                println!("{k}");
            }
            ExitCode::SUCCESS
        }
        ("nom", subargs)
            if !subargs.is_empty()
                && matches!(subargs[0].as_str(), "build" | "shell" | "develop") =>
        {
            forward_exit(Command::new("nix").args(subargs).status())
        }
        _ => {
            println!("No completion support for {} {}", prog, args.join(" "));
            ExitCode::FAILURE
        }
    }
}

fn help_text() -> String {
    [
        "nix-output-monitor usages:",
        "  Wrappers:",
        "    nom build <nix-args>",
        "    nom shell <nix-args>",
        "    nom develop <nix-args>",
        "",
        "    nom-build <nix-args>",
        "    nom-shell <nix-args>",
        "",
        "  Direct piping:",
        "    via json parsing:",
        "      nix build --log-format internal-json -v <nix-args> |& nom --json",
        "      nix-build --log-format internal-json -v <nix-args> |& nom --json",
        "",
        "    via human-readable log parsing:",
        "      nix-build |& nom",
        "",
        "    Don't forget to redirect stderr, too. That's what the & does.",
        "",
        "Flags:",
        "  --version  Show version.",
        "  -h, --help Show this help.",
        "  --json     Parse input as nix internal-json",
    ]
    .join("\n")
}
