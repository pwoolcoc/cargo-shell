extern crate rustyline;
extern crate cargo;
#[macro_use] extern crate error_chain;
#[macro_use] extern crate log;

mod errors;

use std::fs::File;
use std::io::{stderr, Write, BufReader, BufRead};
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::env;

use rustyline::Editor;
use rustyline::error::ReadlineError;
use cargo::core::Package;
use cargo::util::Config as CargoConfig;
use cargo::util::important_paths::{find_root_manifest_for_wd};

use errors::*;

const USAGE: &'static str = r#"Cargo Command Shell
-------------------

Any command that you would normally type after `cargo ` is a valid command here, and should
bring about the same result that running `cargo COMMAND` would from your regular command shell.

Special commands:

  * `+ <command>`
    runs the command under multiple toolchains, which are defined using the `cargo-shell.toolchains`
    configuration option
  * `++ <toolchain> [<command>]`
    This runs a command under a specific toolchain. If the `<command>` is left off, then the active
    toolchain for the shell is changed.
  * `< <filename>`
    This runs commands from the file named by `<filename>`. It looks for a command on each line, and
    lines that are empty or that start with `#` are ignored.
  * `~ <command>`
    This command is only available if `cargo-watch` is available. It will run the `<command>` using
    `cargo-watch`, which causes the command to be re-run whenever a source file changes.

"#;

// TODO: this should come from rustup instead of being specified here
const DEFAULT_TOOLCHAIN: &'static str = "stable";

struct Config {
    pub prompt: String,
    pub rustup: PathBuf,
    pub name: String,
    pub version: String,
    pub default_toolchain: String,
    pub toolchains: Vec<String>,
    pub current_toolchain: String,
    pub cwd: PathBuf,
}

impl Config {
    fn prompt(cconfig: &CargoConfig) -> Result<String> {
        let prompt= cconfig.get_string("cargo-shell.prompt").chain_err(|| "Could not find cargo-shell.prompt")?;
        let prompt = match prompt {
            Some(prompt) => prompt.val,
            None => ">> ".to_string()
        };

        Ok(prompt)
    }

    fn get_prompt(&self) -> String {
        let prompt = self.prompt.replace("{project}", &self.name)
                                .replace("{version}", &self.version)
                                .replace("{toolchain}", &self.current_toolchain);

        prompt
    }

    fn default_toolchain(cconfig: &CargoConfig) -> Result<String> {
        let def = cconfig.get_string("cargo-shell.default-toolchain").chain_err(|| "Could not find cargo-shell.default-toolchain")?;
        let def = match def {
            Some(d) => d.val,
            None => DEFAULT_TOOLCHAIN.into(),
        };
        Ok(def)
    }

    fn get_name_and_version(cconfig: &CargoConfig) -> Result<(String, String)> {
        let manifest = find_root_manifest_for_wd(None, cconfig.cwd()).chain_err(|| "Could not find root manifest for project")?;
        let pkg = Package::for_path(&manifest, cconfig).chain_err(|| "Could not get package path for current crate")?;
        Ok((pkg.name().into(), pkg.version().to_string()))
    }

    fn get_toolchains(cconfig: &CargoConfig) -> Result<Vec<String>> {
        let toolchains = cconfig.get_list("cargo-shell.toolchains").chain_err(|| "Could not get cargo-shell.toolchains value")?;
        let toolchains = match toolchains {
            Some(toolchains) => toolchains.val.into_iter().map(|(s, _p)| s).collect::<Vec<_>>(),
            None => vec!["stable".into(), "beta".into(), "nightly".into()],
        };
        Ok(toolchains)
    }

    fn find_rustup() -> Result<PathBuf> {
        let cargo_home = env::var("CARGO_HOME").chain_err(|| "CARGO_HOME environment variable not set")?;
        let rustup = Path::new(&cargo_home).join("bin").join("rustup");
        if rustup.exists() {
            return Ok(rustup.into());
        } else {
            // I'll need a solution for windows here, too
            let path = env::var("PATH").chain_err(|| "PATH environment variable not set")?;
            let paths = path.split(':');
            for p in paths {
                let rustup = Path::new(p).join("rustup");
                if rustup.exists() {
                    return Ok(rustup.into());
                }
            }
        }
        bail!("Could not find a rustup binary");
    }

    fn new() -> Result<Config> {
        let cconfig = CargoConfig::default().chain_err(|| "Could not get default CargoConfig")?;
        let (name, version) = Config::get_name_and_version(&cconfig)?;
        let prompt = Config::prompt(&cconfig)?;

        let rustup = Config::find_rustup().chain_err(|| "Could not find a `rustup` binary")?;
        debug!("rustup binary found at {:?}", rustup.to_string_lossy());

        let default_toolchain = Config::default_toolchain(&cconfig)?;

        let toolchains = Config::get_toolchains(&cconfig)?;

        Ok(Config {
            prompt: prompt,
            rustup: rustup.into(),
            name: name,
            version: version,
            default_toolchain: default_toolchain.clone(),
            toolchains: toolchains,
            current_toolchain: default_toolchain.clone(),
            cwd: cconfig.cwd().into(),
        })
    }
}

pub fn main() -> Result<()> {
    let v = env!("CARGO_PKG_VERSION");
    println!("Welcome to cargo-shell v{}", v);
    let mut rl = Editor::<()>::new();
    let mut config = Config::new()?;

    loop {
        let line = rl.readline(&config.get_prompt());
        match line {
            Ok(line) => {
                if let Err(e) = dispatch_cmd(&mut config, &line.trim()) {
                    println!("Error: {:?}", e);
                };
            },
            Err(ReadlineError::Eof) => break,
            Err(ReadlineError::Interrupted) => continue,
            Err(e) => println!("No Input: {:?}", e),
        }
    }

    Ok(())
}

fn dispatch_cmd(config: &mut Config, cmd: &str) -> Result<()> {
    if cmd == "exit" || cmd == "quit" {
        ::std::process::exit(0);
    } else if cmd == "help" {
        print_help();
    } else if cmd.starts_with("p ") {
        let p = cmd[2..].trim_matches(|c| c == '"' || c == '\'' ).to_string();
        config.prompt = p;
    } else if cmd.starts_with("~") {
        // ~command
        // run every time a source file changes
        // only available if cargo-watch is installed
        let has_cargo_watch = match Command::new("cargo")
                                      .arg("watch")
                                      .arg("--help")
                                      .stdout(Stdio::null())
                                      .stdin(Stdio::null())
                                      .stderr(Stdio::null())
                                      .status() {
            Ok(status) => status.success(),
            _ => false,
        };
        if !has_cargo_watch {
            let stderr = stderr();
            let _ = writeln!(stderr.lock(),
                    "Could not find cargo-watch, you might need to install it?");
        } else {
            let mut new_cmd = vec!["watch"];
            new_cmd.extend_from_slice(&cmd[1..].trim().split(' ').collect::<Vec<_>>());
            run(config, &new_cmd)?;
        }
    } else if cmd.starts_with("<") {
        // < filename
        // run commands from file `filename`
        let file = &cmd[1..].trim();
        let file = File::open(file).chain_err(|| format!("Could not open filename {}", file))?;
        let file = BufReader::new(file);
        for line in file.lines() {
            let line = line.chain_err(|| "Could not get next line from file")?;
            let line = line.trim();
            if line == "" || line.starts_with("#") {
                continue;
            }
            let line = line.split(' ').collect::<Vec<_>>();
            println!("want to run {:?}?", line);
            //run(config, &line)?;
        }
    } else if cmd.starts_with("++") {
        // ++ <version> <command>
        // temporarily change the version of rust used to run commands
        let parts = cmd[2..].trim().split(' ').collect::<Vec<_>>();
        let version = parts[0].trim();
        let original = config.current_toolchain.clone();
        config.current_toolchain = version.into();
        // the command is actually optional, and will cause the toolchain switch to be temporary
        if parts.len() > 1 {
            let _ = run(config, &parts[1..])?;
            config.current_toolchain = original;
        }
    } else if cmd.starts_with("+") {
        // + <command>
        // run the command across all rust versions specified in the
        // `toolchains` setting list
        let original = config.current_toolchain.clone();
        let args = cmd[1..].trim().split(' ').collect::<Vec<_>>();
        let toolchains = config.toolchains.clone();
        for toolchain in toolchains {
            config.current_toolchain = toolchain;
            println!("Running command with toolchain `{}`", config.current_toolchain);
            run(config, &args)?;
        }
        config.current_toolchain = original;
    } else {
        let args = cmd.split(' ').collect::<Vec<_>>();
        run(config, &args)?;
    }
    Ok(())
}

fn print_help() {
    println!("{}", USAGE);
}

fn run(config: &Config, cmd: &[&str]) -> Result<()> {
    debug!("{} run {} cargo {}",
                &config.rustup.to_string_lossy(),
                &config.default_toolchain,
                cmd.join(" "));
    let _ = Command::new(&config.rustup)
                        .arg("run")
                        .arg(&config.default_toolchain)
                        .arg("cargo")
                        .args(cmd)
                        .current_dir(&config.cwd)
                        .status()
                        .chain_err(|| "Could not execute rustup run command")?;
    Ok(())
}
