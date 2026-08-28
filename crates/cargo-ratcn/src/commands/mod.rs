mod add;
mod init;

use std::path::Path;

use anyhow::Result;

use crate::cli::Command;

pub(crate) fn execute(command: Command, cwd: &Path) -> Result<()> {
    match command {
        Command::Init => init::execute(cwd),
        Command::Add(args) => add::execute(args, cwd),
    }
}
