//! Generate shell completions from the same parser that handles commands.

use clap::{CommandFactory, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}

pub fn script(shell: CompletionShell) -> String {
    let shell = match shell {
        CompletionShell::Bash => clap_complete::Shell::Bash,
        CompletionShell::Zsh => clap_complete::Shell::Zsh,
        CompletionShell::Fish => clap_complete::Shell::Fish,
        CompletionShell::PowerShell => clap_complete::Shell::PowerShell,
    };
    let mut output = Vec::new();
    clap_complete::generate(shell, &mut crate::Cli::command(), "llm_cli", &mut output);
    String::from_utf8(output).expect("generated completion script is UTF-8")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn bash_completes_subcommand_options_after_a_global_config() {
        let test_script = script(CompletionShell::Bash)
            + r#"
COMP_WORDS=(llm_cli --config custom.toml init --y)
COMP_CWORD=4
_llm_cli llm_cli --y init
[[ " ${COMPREPLY[*]} " == *" --yes " ]] || exit 1
COMP_WORDS=(llm_cli ask question --j)
COMP_CWORD=3
_llm_cli llm_cli --j question
[[ " ${COMPREPLY[*]} " == *" --json " ]] || exit 1
"#;
        let status = std::process::Command::new("bash")
            .args(["-c", &test_script])
            .status()
            .expect("bash is installed");
        assert!(status.success());
    }
}
