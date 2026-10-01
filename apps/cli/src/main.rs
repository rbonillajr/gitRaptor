use clap::Parser;

/// The Git copilot for teams that code with AI agents.
#[derive(Parser)]
#[command(name = "raptor", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!(
        "raptor {} (api {})",
        gitraptor_core::version(),
        gitraptor_core::API_VERSION
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
