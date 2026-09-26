use crate::storage::Doctrine;

// Embed the canonical documents, not a second generated copy of their content.
macro_rules! sources {
    ($($title:literal),+ $(,)?) => {
        [$(
            ($title, include_str!(concat!(
                "../../.agents/skills/doctrine/doctrines/", $title, ".doctrine.md"
            )))
        ),+]
    };
}

pub fn doctrines() -> Vec<Doctrine> {
    sources![
        "boundaries",
        "code",
        "context",
        "cyclomatic-complexity",
        "data",
        "data-processing",
        "debugging",
        "distributed-data",
        "documentation",
        "domain",
        "idempotency",
        "integration-testing",
        "laziness",
        "machine",
        "nimble",
        "pragmatic",
        "scout",
        "sequencing",
        "solid",
        "tactical-strategic",
        "test-seams",
        "testing",
        "worktrees",
    ]
    .into_iter()
    .map(|(title, source)| {
        let (_, markdown) = source
            .split_once("\n---\n")
            .expect("Bundled doctrine must have frontmatter");
        let (heading, body) = markdown
            .trim_start()
            .split_once('\n')
            .expect("Bundled doctrine must have a heading and body");
        assert!(
            heading.starts_with("# "),
            "Bundled doctrine must have an H1"
        );
        Doctrine {
            title: title.into(),
            body: body.trim().into(),
        }
    })
    .collect()
}
