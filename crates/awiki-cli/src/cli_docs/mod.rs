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
            summary: "Current installation, first-use, and supported product surface",
            references: &[
                "README.md",
                "docs/getting-started.md",
                "docs/installation.md",
            ],
        },
        Topic {
            name: "architecture",
            summary: "Overall v2 architecture and command model",
            references: &[
                "docs/architecture/awiki-v2-architecture.md",
                "docs/architecture/awiki-command-v2.md",
                "docs/installation.md",
                "docs/architecture/awiki-skill-architecture.md",
            ],
        },
        Topic {
            name: "identity",
            summary: "Identity registration, recovery, selection, profile, and migration commands",
            references: &[
                "skills/references/02-identity.md",
                "skills/references/01-onboarding.md",
            ],
        },
        Topic {
            name: "messaging",
            summary: "Direct and group messages, attachments, read state, and secure messaging",
            references: &[
                "skills/references/03-messaging.md",
                "skills/references/04-groups.md",
            ],
        },
        Topic {
            name: "groups",
            summary: "Group lifecycle, membership, history, and supported secure operations",
            references: &["skills/references/04-groups.md"],
        },
        Topic {
            name: "tenant",
            summary: "Tenant registry, backend base URL, DID host, and workspace switching",
            references: &[
                "skills/references/13-tenants.md",
                "docs/installation.md",
                "docs/architecture/anp-service-discovery.md",
            ],
        },
        Topic {
            name: "mail",
            summary: "Top-level mail command surface and service configuration",
            references: &[
                "skills/references/12-mail.md",
                "docs/architecture/awiki-mail-cli.md",
            ],
        },
        Topic {
            name: "site",
            summary: "Tenant bare-domain site page commands and contracts",
            references: &[
                "skills/references/11-site-pages.md",
                "docs/architecture/awiki-site-pages.md",
            ],
        },
        Topic {
            name: "people",
            summary: "Relationships, followers, following, and local contacts",
            references: &[
                "skills/references/09-people.md",
                "skills/references/07-discovery.md",
            ],
        },
        Topic {
            name: "pages",
            summary: "Handle-level content pages and tenant site pages",
            references: &[
                "skills/references/06-pages.md",
                "skills/references/11-site-pages.md",
            ],
        },
        Topic {
            name: "skills",
            summary: "Current skill entrypoint and reference topology",
            references: &[
                "skills/SKILL.md",
                "docs/architecture/awiki-skill-architecture.md",
                "skills/references/00-installation.md",
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
            name: "storage",
            summary: "Tenant workspace, identity secrets, and local state ownership",
            references: &[
                "docs/installation.md",
                "docs/architecture/identity-secret-storage.md",
                "docs/architecture/local-state-owner-scope.md",
            ],
        },
        Topic {
            name: "runtime",
            summary: "Runtime mode, listener service, and OpenClaw or Hermes host notifications",
            references: &[
                "skills/references/05-runtime.md",
                "docs/architecture/hermes-host-notify-v1-runbook.md",
                "docs/architecture/openclaw-host-adapter-v1.md",
            ],
        },
    ]
}
