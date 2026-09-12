//! Portable shell-completion scripts for the installed executable.
//!
//! These intentionally avoid a build-time dependency on a shell-completion
//! crate. The generated scripts cover the public subcommands and options, and
//! remain useful even when a person installs a release binary without Cargo.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell", alias = "pwsh")]
    PowerShell,
}

/// Return a completion script that a shell can source or place in its normal
/// completion directory.
pub const fn script(shell: CompletionShell) -> &'static str {
    match shell {
        CompletionShell::Bash => BASH,
        CompletionShell::Zsh => ZSH,
        CompletionShell::Fish => FISH,
        CompletionShell::PowerShell => POWERSHELL,
    }
}

const BASH: &str = r#"# llm_cli Bash completion
_llm_cli() {
    local current previous subcommand
    current="${COMP_WORDS[COMP_CWORD]}"
    previous="${COMP_WORDS[COMP_CWORD-1]}"
    subcommand="${COMP_WORDS[1]}"

    if [[ "$previous" == "--config" ]]; then
        COMPREPLY=( $(compgen -f -- "$current") )
        return 0
    fi

    if [[ "$subcommand" == "completions" ]]; then
        COMPREPLY=( $(compgen -W "bash zsh fish powershell pwsh" -- "$current") )
        return 0
    fi

    if (( COMP_CWORD == 1 )); then
        COMPREPLY=( $(compgen -W "ask audit completions doctor health init run setup --config --help --version" -- "$current") )
        return 0
    fi

    case "$subcommand" in
        ask) COMPREPLY=( $(compgen -W "--json --config --help" -- "$current") ) ;;
        audit) COMPREPLY=( $(compgen -W "--tail --config --help" -- "$current") ) ;;
        doctor) COMPREPLY=( $(compgen -W "--full --config --help" -- "$current") ) ;;
        health) COMPREPLY=( $(compgen -W "--model --full --config --help" -- "$current") ) ;;
        init) COMPREPLY=( $(compgen -W "--full --yes --config --help" -- "$current") ) ;;
        setup) COMPREPLY=( $(compgen -W "--force --config --help" -- "$current") ) ;;
        run) COMPREPLY=( $(compgen -W "--config --help" -- "$current") ) ;;
    esac
}
complete -F _llm_cli llm_cli
"#;

const ZSH: &str = r#"#compdef llm_cli
# llm_cli Zsh completion
_llm_cli() {
  local -a commands common_options
  commands=(
    'ask:ask one read-only question'
    'audit:show recent shell audit entries'
    'completions:generate a shell completion script'
    'doctor:inspect local setup without changing it'
    'health:check Ollama and the selected model'
    'init:download missing local models'
    'run:launch the terminal interface'
    'setup:create a commented project configuration file'
  )
  common_options=(
    '--config=[configuration file]:configuration file:_files'
    '--help[print help]'
  )

  if (( CURRENT == 2 )); then
    _alternative \
      'commands:llm_cli command:_describe -t commands command "$commands"' \
      'options:option:_describe -t options option "$common_options"'
    return
  fi

  case "$words[2]" in
    ask) _arguments "$common_options[@]" '--json[emit one JSON response]' '*:question:' ;;
    audit) _arguments "$common_options[@]" '--tail=[number of entries]:count:' ;;
    completions) _arguments "$common_options[@]" '1:shell:(bash zsh fish powershell pwsh)' ;;
    doctor) _arguments "$common_options[@]" '--full[check optional routing models]' ;;
    health) _arguments "$common_options[@]" '--full[check optional routing models]' '--model=[model name]:model:' ;;
    init) _arguments "$common_options[@]" '--full[download optional routing models]' '--yes[download without confirmation]' ;;
    setup) _arguments "$common_options[@]" '--force[replace an existing configuration file]' ;;
    run) _arguments "$common_options[@]" ;;
  esac
}
_llm_cli "$@"
"#;

const FISH: &str = r#"# llm_cli Fish completion
complete -c llm_cli -f
complete -c llm_cli -l config -r -F -d 'Configuration file'
complete -c llm_cli -s h -l help -d 'Print help'
complete -c llm_cli -s V -l version -d 'Print version'

complete -c llm_cli -n '__fish_use_subcommand' -a ask -d 'Ask one read-only question'
complete -c llm_cli -n '__fish_use_subcommand' -a audit -d 'Show recent shell audit entries'
complete -c llm_cli -n '__fish_use_subcommand' -a completions -d 'Generate shell completion script'
complete -c llm_cli -n '__fish_use_subcommand' -a doctor -d 'Inspect local setup without changing it'
complete -c llm_cli -n '__fish_use_subcommand' -a health -d 'Check Ollama and the selected model'
complete -c llm_cli -n '__fish_use_subcommand' -a init -d 'Download missing local models'
complete -c llm_cli -n '__fish_use_subcommand' -a run -d 'Launch the terminal interface'
complete -c llm_cli -n '__fish_use_subcommand' -a setup -d 'Create a project configuration file'

complete -c llm_cli -n '__fish_seen_subcommand_from ask' -l json -d 'Emit one JSON response'
complete -c llm_cli -n '__fish_seen_subcommand_from audit' -l tail -r -d 'Number of audit entries'
complete -c llm_cli -n '__fish_seen_subcommand_from doctor' -l full -d 'Check optional routing models'
complete -c llm_cli -n '__fish_seen_subcommand_from health' -l full -d 'Check optional routing models'
complete -c llm_cli -n '__fish_seen_subcommand_from health' -l model -r -d 'Model name'
complete -c llm_cli -n '__fish_seen_subcommand_from init' -l full -d 'Download optional routing models'
complete -c llm_cli -n '__fish_seen_subcommand_from init' -l yes -d 'Download without confirmation'
complete -c llm_cli -n '__fish_seen_subcommand_from setup' -l force -d 'Replace existing configuration'
complete -c llm_cli -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish powershell pwsh'
"#;

const POWERSHELL: &str = r#"# llm_cli PowerShell completion
Register-ArgumentCompleter -Native -CommandName llm_cli -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $tokens = @($commandAst.CommandElements | ForEach-Object { $_.Extent.Text })
    $subcommand = if ($tokens.Count -gt 1) { $tokens[1] } else { '' }
    $candidates = switch ($subcommand) {
        'ask' { @('--json', '--config', '--help') }
        'audit' { @('--tail', '--config', '--help') }
        'completions' { @('bash', 'zsh', 'fish', 'powershell', 'pwsh', '--config', '--help') }
        'doctor' { @('--full', '--config', '--help') }
        'health' { @('--model', '--full', '--config', '--help') }
        'init' { @('--full', '--yes', '--config', '--help') }
        'setup' { @('--force', '--config', '--help') }
        'run' { @('--config', '--help') }
        default { @('ask', 'audit', 'completions', 'doctor', 'health', 'init', 'run', 'setup', '--config', '--help', '--version') }
    }

    $candidates |
        Where-Object { $_ -like "$wordToComplete*" } |
        ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
}
"#;

#[cfg(test)]
mod tests {
    use super::{script, CompletionShell};

    #[test]
    fn every_shell_script_registers_llm_cli() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
            CompletionShell::PowerShell,
        ] {
            assert!(script(shell).contains("llm_cli"));
        }
    }

    #[test]
    fn completion_scripts_cover_the_public_commands() {
        let bash = script(CompletionShell::Bash);
        for command in [
            "ask",
            "audit",
            "completions",
            "doctor",
            "health",
            "init",
            "run",
            "setup",
        ] {
            assert!(bash.contains(command));
        }
    }
}
