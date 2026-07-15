use crate::command_catalog::{self, CommandSpec};
use std::collections::BTreeSet;

const GLOBAL_FLAGS: &[&str] = &[
    "--tenant",
    "--identity",
    "--format",
    "--jq",
    "--dry-run",
    "--diagnostic",
    "--migration",
    "--verbose",
    "--help",
];

pub fn render(shell: &str) -> String {
    let nodes = completion_nodes();
    let value_flags = value_flags();
    match shell {
        "bash" => render_bash(&nodes, &value_flags),
        "zsh" => render_zsh(&nodes, &value_flags),
        "fish" => render_fish(&nodes, &value_flags),
        "powershell" => render_powershell(&nodes, &value_flags),
        _ => String::new(),
    }
}

#[derive(Debug)]
struct CompletionNode {
    path: String,
    candidates: Vec<String>,
}

fn completion_nodes() -> Vec<CompletionNode> {
    let roots = command_catalog::public_help_root_specs();
    let mut nodes = vec![CompletionNode {
        path: String::new(),
        candidates: candidates(None, &roots),
    }];
    for root in roots {
        append_nodes(&mut nodes, &root);
    }
    nodes
}

fn append_nodes(nodes: &mut Vec<CompletionNode>, spec: &CommandSpec) {
    let children = command_catalog::public_help_children_of(spec.name);
    nodes.push(CompletionNode {
        path: spec.name.replace('.', " "),
        candidates: candidates(Some(spec), &children),
    });
    for child in children {
        append_nodes(nodes, &child);
    }
}

fn candidates(spec: Option<&CommandSpec>, children: &[CommandSpec]) -> Vec<String> {
    let mut values = BTreeSet::new();
    for child in children {
        if let Some(name) = child.use_.split_whitespace().next() {
            values.insert(name.to_string());
        }
    }
    if let Some(spec) = spec {
        for flag in spec.flags.iter().filter(|flag| !flag.deprecated) {
            values.insert(format!("--{}", flag.name));
        }
    }
    values.extend(GLOBAL_FLAGS.iter().map(|flag| (*flag).to_string()));
    values.into_iter().collect()
}

fn value_flags() -> Vec<String> {
    let mut values: BTreeSet<String> = ["tenant", "identity", "format", "jq"]
        .into_iter()
        .map(|name| format!("--{name}"))
        .collect();
    for spec in command_catalog::specs() {
        for flag in spec.flags {
            if flag.flag_type != "bool" {
                values.insert(format!("--{}", flag.name));
            }
        }
    }
    values.into_iter().collect()
}

fn render_bash(nodes: &[CompletionNode], value_flags: &[String]) -> String {
    let mut out = format!(
        "_awiki_cli() {{\n  local cur path word i skip_next\n  cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n  path=\"\"\n  skip_next=0\n  for ((i=1; i<COMP_CWORD; i++)); do\n    word=\"${{COMP_WORDS[i]}}\"\n    if ((skip_next)); then skip_next=0; continue; fi\n    case \"$word\" in\n      {}) skip_next=1; continue ;;\n      --*=*|--*) continue ;;\n    esac\n    path=\"${{path:+$path }}$word\"\n  done\n  local candidates=\"\"\n  case \"$path\" in\n",
        value_flags.join("|")
    );
    append_posix_cases(&mut out, nodes, "    ");
    out.push_str(
        "  esac\n  COMPREPLY=( $(compgen -W \"$candidates\" -- \"$cur\") )\n}\ncomplete -F _awiki_cli awiki-cli\n",
    );
    out
}

fn render_zsh(nodes: &[CompletionNode], value_flags: &[String]) -> String {
    let mut out = format!(
        "#compdef awiki-cli\n_awiki_cli() {{\n  local path=\"\" word candidates=\"\"\n  integer i skip_next=0\n  for ((i=2; i<CURRENT; i++)); do\n    word=\"${{words[i]}}\"\n    if ((skip_next)); then skip_next=0; continue; fi\n    case \"$word\" in\n      {}) skip_next=1; continue ;;\n      --*=*|--*) continue ;;\n    esac\n    path=\"${{path:+$path }}$word\"\n  done\n  case \"$path\" in\n",
        value_flags.join("|")
    );
    append_posix_cases(&mut out, nodes, "    ");
    out.push_str("  esac\n  compadd -- ${(z)candidates}\n}\ncompdef _awiki_cli awiki-cli\n");
    out
}

fn append_posix_cases(out: &mut String, nodes: &[CompletionNode], indent: &str) {
    for node in nodes {
        out.push_str(indent);
        out.push('\'');
        out.push_str(&node.path.replace('\'', "'\\''"));
        out.push_str("') candidates='");
        out.push_str(&node.candidates.join(" "));
        out.push_str("' ;;\n");
    }
}

fn render_fish(nodes: &[CompletionNode], value_flags: &[String]) -> String {
    let mut out = String::from(
        "function __awiki_cli_candidates\n  set -l tokens (commandline -opc)\n  if test (count $tokens) -gt 0\n    set -e tokens[1]\n  end\n  set -l path\n  set -l skip_next 0\n  for word in $tokens\n    if test $skip_next -eq 1\n      set skip_next 0\n      continue\n    end\n    switch $word\n",
    );
    out.push_str("      case ");
    out.push_str(&value_flags.join(" "));
    out.push_str(
        "\n        set skip_next 1\n      case '--*=*' '--*'\n        continue\n      case '*'\n        set -a path $word\n    end\n  end\n  switch (string join ' ' $path)\n",
    );
    for node in nodes {
        out.push_str("    case '");
        out.push_str(&node.path.replace('\'', "\\'"));
        out.push_str("'\n      printf '%s\\n' ");
        out.push_str(&node.candidates.join(" "));
        out.push('\n');
    }
    out.push_str("  end\nend\ncomplete -c awiki-cli -f -a '(__awiki_cli_candidates)'\n");
    out
}

fn render_powershell(nodes: &[CompletionNode], value_flags: &[String]) -> String {
    let mut out = String::from(
        "Register-ArgumentCompleter -Native -CommandName awiki-cli -ScriptBlock {\n  param($wordToComplete, $commandAst, $cursorPosition)\n  $tokens = @($commandAst.CommandElements | Select-Object -Skip 1 | ForEach-Object { $_.Extent.Text })\n  if ($tokens.Count -gt 0 -and $tokens[-1] -eq $wordToComplete) { $tokens = @($tokens | Select-Object -SkipLast 1) }\n  $path = @()\n  $skipNext = $false\n  foreach ($word in $tokens) {\n    if ($skipNext) { $skipNext = $false; continue }\n    if (@('",
    );
    out.push_str(&value_flags.join("','"));
    out.push_str(
        "') -contains $word) { $skipNext = $true; continue }\n    if ($word.StartsWith('--')) { continue }\n    $path += $word\n  }\n  $candidates = switch ($path -join ' ') {\n",
    );
    for node in nodes {
        out.push_str("    '");
        out.push_str(&node.path.replace('\'', "''"));
        out.push_str("' { @('");
        out.push_str(&node.candidates.join("','"));
        out.push_str("'); break }\n");
    }
    out.push_str(
        "    default { @() }\n  }\n  $candidates | Where-Object { $_ -like \"$wordToComplete*\" } | ForEach-Object { [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_) }\n}\n",
    );
    out
}
