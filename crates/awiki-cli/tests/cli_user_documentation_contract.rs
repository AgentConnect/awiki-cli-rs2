use awiki_cli::command_catalog::{self, DirectInvocationPolicy};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn skill_confirmation_rules_cover_every_public_side_effect() {
    let root = repo_root();
    let skill = fs::read_to_string(root.join("skills/SKILL.md")).expect("read Skill entrypoint");
    let missing: Vec<_> = command_catalog::specs()
        .into_iter()
        .filter(|spec| {
            spec.side_effect
                && !spec.hidden
                && spec.direct_invocation() == DirectInvocationPolicy::Allow
        })
        .filter_map(|spec| {
            let documented = format!("`{}`", spec.name.replace('.', " "));
            (!skill.contains(&documented)).then_some(spec.name)
        })
        .collect();

    assert!(
        missing.is_empty(),
        "skills/SKILL.md confirmation rules must cover all public side-effecting commands: {missing:?}"
    );
}

#[test]
fn user_command_examples_include_required_diagnostic_and_migration_gates() {
    let root = repo_root();
    let mut docs = markdown_files_under(&root.join("skills"));
    docs.extend([
        root.join("onboarding.md"),
        root.join("README.md"),
        root.join("README.zh-CN.md"),
        root.join("docs/getting-started.md"),
        root.join("docs/getting-started.zh-CN.md"),
        root.join("docs/installation.md"),
    ]);

    let gated_commands = [
        ("id import-v1", "--migration"),
        ("id replace-did", "--diagnostic"),
        ("debug db handle-history", "--diagnostic"),
        ("debug db import-v1", "--migration"),
    ];
    let mut violations = Vec::new();

    for path in docs {
        let text = fs::read_to_string(&path).expect("read user-facing markdown");
        let normalized = text.replace("\\\n", " ");
        for (index, line) in normalized.lines().enumerate() {
            for invocation in line.split("awiki-cli").skip(1) {
                for (command, gate) in gated_commands {
                    let Some(command_index) = invocation.find(command) else {
                        continue;
                    };
                    if invocation[..command_index].contains("schema") {
                        continue;
                    }
                    if !invocation[..command_index].contains(gate) {
                        violations.push(format!(
                            "{}:{} awiki-cli {command} requires {gate}",
                            path.strip_prefix(&root).unwrap_or(&path).display(),
                            index + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "user-facing gated command examples would be rejected by the CLI:\n{}",
        violations.join("\n")
    );
}

#[test]
fn skill_token_onboarding_docs_require_closing_stdin_after_the_token_line() {
    let root = repo_root();
    for relative in ["onboarding.md", "skills/references/01-onboarding.md"] {
        let text = fs::read_to_string(root.join(relative)).expect("read onboarding guide");
        assert!(
            text.contains("EOF") && text.contains("close"),
            "{relative} must tell Agent tools to close stdin with EOF after the Token line"
        );
    }
}

#[test]
fn skill_router_references_exist() {
    let root = repo_root();
    let skill = fs::read_to_string(root.join("skills/SKILL.md")).expect("read Skill entrypoint");
    for reference in [
        "references/00-installation.md",
        "references/01-onboarding.md",
        "references/02-identity.md",
        "references/03-messaging.md",
        "references/04-groups.md",
        "references/05-runtime.md",
        "references/06-pages.md",
        "references/07-discovery.md",
        "references/08-debug.md",
        "references/09-people.md",
        "references/10-upgrade.md",
        "references/11-site-pages.md",
        "references/12-mail.md",
        "references/13-tenants.md",
    ] {
        assert!(
            skill.contains(reference),
            "Skill router is missing {reference}"
        );
        assert!(
            root.join("skills").join(reference).is_file(),
            "Skill router target does not exist: {reference}"
        );
    }
}

#[test]
fn skill_docs_do_not_advertise_non_product_or_compatibility_commands() {
    let root = repo_root();
    let docs = markdown_files_under(&root.join("skills"));
    let forbidden = [
        "runtime heartbeat",
        "people search",
        "debug db query",
        "debug raw rpc",
        "debug schema-cache",
        "debug logs",
        "msg secure init",
        "msg secure failed",
        "msg secure retry",
        "msg secure drop",
        "group e2ee publish-key-package",
        "group e2ee pending",
        "group e2ee process-leave-request",
        "group e2ee recover-member",
        "group e2ee update-key",
        "group e2ee rejoin",
        "awiki-cli id create",
        "--message-security-profile",
        "--e2ee",
    ];
    let mut violations = Vec::new();

    for path in docs {
        let text = fs::read_to_string(&path).expect("read Skill markdown");
        for command in forbidden {
            if text.contains(command) {
                violations.push(format!(
                    "{} advertises {command:?}",
                    path.strip_prefix(&root).unwrap_or(&path).display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Skill docs should describe only the current product surface:\n{}",
        violations.join("\n")
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under workspace root")
        .to_path_buf()
}

fn markdown_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read documentation directory") {
            let entry = entry.expect("read documentation entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }
    files
}
