mod cli;
mod commands;
mod component_copy;
mod project;

use std::{env, ffi::OsStr};

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::Cli;

/// Runs the command selected by Cargo's subcommand invocation.
pub fn run() -> Result<()> {
    let mut arguments: Vec<_> = env::args_os().collect();
    // Cargo forwards the external subcommand name as argv[1], while a direct
    // cargo-ratcn invocation starts with the command itself.
    if arguments
        .get(1)
        .is_some_and(|argument| argument == OsStr::new("ratcn"))
    {
        arguments.remove(1);
    }
    let cli = Cli::parse_from(arguments);
    let cwd = env::current_dir().context("could not determine the current directory")?;

    commands::execute(cli.command, &cwd)
}
