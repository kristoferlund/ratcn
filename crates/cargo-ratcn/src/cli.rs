use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cargo-ratcn",
    bin_name = "cargo ratcn",
    about = "Manage ratcn components",
    arg_required_else_help = true,
    subcommand_required = true
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Initialize ratcn in the current project.
    Init,
    /// Add ratcn components to the current project.
    Add(AddArgs),
}

#[derive(Args, Debug)]
pub(crate) struct AddArgs {
    /// Components to add.
    #[arg(value_name = "COMPONENT", num_args = 1.., required_unless_present = "list")]
    pub(crate) components: Vec<String>,

    /// List available components.
    #[arg(long, conflicts_with = "components")]
    pub(crate) list: bool,

    /// Replace existing component files.
    #[arg(long)]
    pub(crate) force: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{AddArgs, Cli, Command};

    #[test]
    fn init_accepts_no_arguments() {
        let cli = Cli::try_parse_from(["cargo-ratcn", "init"]).expect("init takes no arguments");

        assert!(matches!(cli.command, Command::Init));
        assert!(Cli::try_parse_from(["cargo-ratcn", "init", "--browser"]).is_err());
    }

    #[test]
    fn add_accepts_one_or_more_components() {
        let cli = Cli::try_parse_from(["cargo-ratcn", "add", "button", "tabs", "--force"])
            .expect("one or more components should parse");

        assert!(matches!(
            cli.command,
            Command::Add(AddArgs {
                components,
                list: false,
                force: true,
            }) if components == ["button", "tabs"]
        ));
    }

    #[test]
    fn add_requires_components_unless_listing() {
        assert!(Cli::try_parse_from(["cargo-ratcn", "add"]).is_err());
        assert!(Cli::try_parse_from(["cargo-ratcn", "add", "button", "--list"]).is_err());
        assert!(Cli::try_parse_from(["cargo-ratcn", "add", "--list"]).is_ok());
    }
}
