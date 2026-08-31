mod add;
mod init;

use std::path::Path;

use anyhow::{Context, Result};
use console::style;

use crate::cli::Command;

fn intro(command: &str) -> Result<()> {
    cliclack::intro(style(format!(" {command} ")).on_cyan().black())
        .context("could not start command output")
}

pub(crate) fn execute(command: Command, cwd: &Path) -> Result<()> {
    match command {
        Command::Init => init::execute(cwd),
        Command::Add(args) => add::execute(args, cwd),
    }
}
