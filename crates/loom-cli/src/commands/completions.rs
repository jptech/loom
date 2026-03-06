use clap::{Args, CommandFactory};
use clap_complete::{generate, Shell};

use loom_core::error::LoomError;

use crate::GlobalContext;

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,
}

pub fn run(args: CompletionsArgs, _ctx: &GlobalContext) -> Result<(), LoomError> {
    let mut cmd = crate::Cli::command();
    generate(args.shell, &mut cmd, "loom", &mut std::io::stdout());
    Ok(())
}
