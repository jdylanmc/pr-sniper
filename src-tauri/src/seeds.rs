use crate::storage::{Doctrine, Voice};

// Shipped user libraries. Skill doctrines under `.agents/skills/doctrine` are a
// separate catalog and must not be embedded here.
macro_rules! sources {
    ($dir:literal, $ext:literal: $($title:literal),+ $(,)?) => {
        [$(
            ($title, include_str!(concat!(
                "../", $dir, "/", $title, ".", $ext, ".md"
            )))
        ),+]
    };
}

/// The body below the frontmatter and H1 of a bundled library entry.
fn body(kind: &str, source: &str) -> String {
    let (_, markdown) = source
        .split_once("\n---\n")
        .unwrap_or_else(|| panic!("Bundled {kind} must have frontmatter"));
    let (heading, body) = markdown
        .trim_start()
        .split_once('\n')
        .unwrap_or_else(|| panic!("Bundled {kind} must have a heading and body"));
    assert!(heading.starts_with("# "), "Bundled {kind} must have an H1");
    body.trim().into()
}

pub fn doctrines() -> Vec<Doctrine> {
    sources![
        "doctrines", "doctrine":
        "bounded-context",
        "code",
        "cyclomatic-complexity",
        "data",
        "documentation",
        "laziness",
        "machine",
        "solid",
        "tactical-strategic",
        "testing",
    ]
    .into_iter()
    .map(|(title, source)| Doctrine {
        title: title.into(),
        body: body("doctrine", source),
    })
    .collect()
}

/// Seeded once into a library that has never held Voices; never topped up.
pub fn voices() -> Vec<Voice> {
    sources!["voices", "voice": "caveman", "gilfoyle", "merlin"]
        .into_iter()
        .map(|(slug, source)| Voice {
            title: slug[..1].to_uppercase() + &slug[1..],
            body: body("voice", source),
            guardrails: true,
        })
        .collect()
}
