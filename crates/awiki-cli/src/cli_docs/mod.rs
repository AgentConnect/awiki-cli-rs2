use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Topic {
    pub name: &'static str,
    pub summary: &'static str,
    pub references: &'static [&'static str],
}

pub fn all() -> Vec<Topic> {
    topics().to_vec()
}

pub fn lookup(name: &str) -> Option<Topic> {
    let needle = name.trim().to_ascii_lowercase();
    topics()
        .iter()
        .find(|topic| topic.name.eq_ignore_ascii_case(&needle))
        .cloned()
}

fn topics() -> &'static [Topic] {
    &[
        Topic {
            name: "overview",
            summary: "Project-level implementation overview and roadmap",
            references: &["docs/plan/awiki-v2-implementation-plan.md"],
        },
        Topic {
            name: "phase-0",
            summary: "Frozen implementation constraints and audit outputs",
            references: &[
                "docs/plan/phase-0/implementation-constraints.md",
                "docs/plan/phase-0/capability-mapping.md",
                "docs/plan/phase-0/audit-findings.md",
                "docs/plan/phase-0/adr-index.md",
            ],
        },
        Topic {
            name: "architecture",
            summary: "Overall v2 architecture and command model",
            references: &[
                "docs/architecture/awiki-v2-architecture.md",
                "docs/architecture/awiki-command-v2.md",
                "docs/installation.md",
                "docs/architecture/awiki-mail-cli.md",
                "docs/architecture/awiki-skill-architecture.md",
            ],
        },
        Topic {
            name: "tenant",
            summary: "Tenant registry, backend base URL, DID host, and workspace switching",
            references: &[
                "docs/installation.md",
                "docs/architecture/awiki-command-v2.md",
                "docs/architecture/anp-service-discovery.md",
            ],
        },
        Topic {
            name: "mail",
            summary: "Top-level mail command surface and service configuration",
            references: &[
                "docs/architecture/awiki-mail-cli.md",
                "docs/architecture/awiki-v2-architecture.md",
                "docs/plan/phase-0/implementation-constraints.md",
            ],
        },
        Topic {
            name: "site",
            summary: "Tenant bare-domain site page commands and contracts",
            references: &[
                "docs/architecture/awiki-site-pages.md",
                "skills/references/11-site-pages.md",
                "docs/architecture/awiki-command-v2.md",
                "docs/architecture/awiki-v2-architecture.md",
            ],
        },
        Topic {
            name: "skills",
            summary: "Current skill entrypoint and reference topology",
            references: &[
                "skills/SKILL.md",
                "docs/architecture/awiki-skill-architecture.md",
                "skills/references/02-identity.md",
                "skills/references/03-messaging.md",
                "skills/references/12-notify.md",
            ],
        },
        Topic {
            name: "output",
            summary: "Output contract, dry-run, schema, and exit code rules",
            references: &["docs/architecture/output-format.md"],
        },
        Topic {
            name: "review",
            summary: "Secondary review checklist and dependency map for PR review",
            references: &[
                "docs/harness/review-spec.md",
                "docs/plan/phase-0/implementation-constraints.md",
                "docs/plan/phase-0/audit-findings.md",
            ],
        },
        Topic {
            name: "storage",
            summary: "Identity layout and SQLite baseline references",
            references: &[
                "docs/plan/phase-0/implementation-constraints.md",
                "../awiki-agent-id-message/scripts/credential_layout.py",
                "../awiki-agent-id-message/scripts/local_store.py",
                "../awiki-agent-id-message/references/local-store-schema.md",
            ],
        },
        Topic {
            name: "runtime",
            summary: "Runtime mode, listener, heartbeat, and migration references",
            references: &[
                "docs/architecture/awiki-v2-architecture.md",
                "../awiki-agent-id-message/scripts/setup_realtime.py",
                "../awiki-agent-id-message/scripts/ws_listener.py",
                "../awiki-agent-id-message/references/WEBSOCKET_LISTENER.md",
            ],
        },
    ]
}
