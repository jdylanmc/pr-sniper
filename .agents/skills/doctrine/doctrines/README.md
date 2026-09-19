# Engineering Doctrine

Doctrine files capture this repository's human-authored, opinionated
engineering philosophy for how software should be shaped. They state reasoned
positions - for example, that a unit test asserts behavior rather than
implementation - rather than generic industry best practices. Skills apply the
relevant positions selectively so that review reflects that judgment
consistently rather than the preferences of whoever is looking.

The canonical current doctrine inventory is [`manifest.md`](manifest.md). Each
entry records a doctrine ID, file path, and integrity hash; the manifest, rather
than a copied list here, defines the trusted set.

## Authorship

Doctrine is written by humans. An agent must not create, edit, reword, or delete
a doctrine file, or change a `manifest.md` entry, unless the operator explicitly
asks for that specific change. An agent may always argue in prose that a rule is
wrong, missing, or ambiguous; it may not act on that opinion by editing the file.

Because doctrine is applied systematically, editing a rule changes the outcome
of every review that rule touches, including work already reviewed under the
previous wording. Treat a doctrine edit as the highest-consequence change in the
repository and make it deliberately.

Git history is the changelog. There are no version fields, changelog files, or
per-rule revision metadata, and none should be added.

## Integrity

`manifest.md` defines the canonical file identities and integrity hashes. After
editing a doctrine file, run `shasum -a 256 .agents/skills/doctrine/doctrines/<id>.doctrine.md` from the
repository root and replace the matching digest in `manifest.md`.

The digest is a tamper boundary, not a review. It proves a file is the one the
manifest names, so a consumer refuses to load doctrine it cannot verify. It says
nothing about whether the change was a good idea. Re-hashing an edit is not
approval of it.

## Applying doctrine

Use the [Doctrine skill](../SKILL.md) to list, select, and load sources through
one common interface. Catalog and worker-selection responses contain metadata
only; the applying agent retrieves verified full text. See the shared
[application contract](../APPLY.md) for required selections and work packets.

- use only rules relevant to the current task and reviewer lens;
- treat repository evidence and requirements as authoritative;
- cite the exact rule a recommendation came from, never doctrine in general;
- report only what the supplied evidence grounds, and carry a confidence,
  because these positions are opinionated and not always decidable by
  inspection;
- produce recommendations for a human to weigh. Applying doctrine does not
  automatically pass or block anything;
- arbitrate overlap explicitly rather than loading every rule indiscriminately.
