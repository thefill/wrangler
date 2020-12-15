use std::env;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::terminal::message::{Message, StdOut};

use super::ScriptFormat;

const BUILD_DIR: &str = "dist";
const SRC_DIR: &str = "src";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    pub build_command: Option<String>,
    #[serde(default = "build_dir")]
    pub build_dir: PathBuf,
    pub upload_format: ScriptFormat,
    #[serde(default = "src_dir")]
    pub src_dir: PathBuf,
}

fn default_warning(field: &str, default: &str) {
    StdOut::warn(&format!(
        "{} not specified, falling back to {}",
        field, default
    ));
}

fn build_dir() -> PathBuf {
    default_warning("build dir", BUILD_DIR);
    let current_dir = env::current_dir().unwrap();
    current_dir.join(BUILD_DIR)
}

fn src_dir() -> PathBuf {
    default_warning("src dir", SRC_DIR);
    let current_dir = env::current_dir().unwrap();
    current_dir.join(SRC_DIR)
}

impl Builder {
    pub fn build_command(&self) -> Option<Command> {
        if let Some(cmd) = &self.build_command {
            let args: Vec<&str> = cmd.split_whitespace().collect();

            let command = if cfg!(target_os = "windows") {
                let mut c = Command::new("cmd");
                c.arg("/C").args(args.as_slice());
                c
            } else {
                let mut c = Command::new(args[0]);
                if args.len() > 1 {
                    c.args(&args[1..]);
                }
                c
            };

            Some(command)
        } else {
            None
        }
    }
}
