# Third-Party Notices

Original repository material is MIT-licensed; see [LICENSE](./LICENSE).
Imported collections retain their own licenses and attribution. The repository
license does not replace those terms.

## Imported collections

| Source | Copyright | License |
| --- | --- | --- |
| [mattpocock/skills](https://github.com/mattpocock/skills) | 2026 Matt Pocock | [MIT](./licenses/mattpocock-skills.LICENSE) |
| [juliusbrussee/caveman](https://github.com/juliusbrussee/caveman) | 2026 Julius Brussee | [MIT for skills](./licenses/caveman.LICENSE) |
| [anthropics/skills](https://github.com/anthropics/skills), historical `skill-creator` import (removed) | 2026 Anthropic, PBC. | [Apache-2.0](./licenses/anthropic-skills.LICENSE) |
| [obra/superpowers](https://github.com/obra/superpowers) | 2025 Jesse Vincent | [MIT](./licenses/superpowers.LICENSE) |

`provenance/skills-lock.json` maps imported skills' local names to their primary upstream sources and
imported content hashes, not hashes of local adaptations. Additional sources
for consolidated skills are recorded below. Imported material is not relicensed
as original work.

Caveman uses split licensing. Its `skills/` directory is MIT-licensed; its
engine-linked runtime has separate terms. This import contains skills, not
that runtime. The upstream [licensing scope](./licenses/caveman.LICENSING.md)
is retained alongside its license.

Preserve applicable notices when copying, renaming, or adapting a skill.
Modifications to Apache-licensed files must carry prominent change notices.

## Consolidated skills

- `conflicts` renames Matt Pocock's `resolving-merge-conflicts`, retaining its
  original import source path/hash. The Matt Pocock MIT notice applies.
- `automate-this` renames Matt Pocock's `loop-me`, retaining its workflow-design
  behavior and original import source/hash. The Matt Pocock MIT notice applies.
- `squadron` renames Jesse Vincent's `dispatching-parallel-agents`, retaining
  its independent-task and worker-packet foundations and original import
  source/hash. Its workflow now covers bounded delivery and Shepherd
  assignments, isolated writes, and explicit custody. The Superpowers MIT
  notice applies. It does not restore the archived Squadron implementations.
- `evolve-architecture` adapts Matt Pocock's `improve-codebase-architecture`
  around the operator-approved evolution workflow and newly requested intent.
  It retains evidenced hotspot analysis and useful visual comparisons, not
  the mandatory HTML/CDN scaffold or a blanket deepening preference. Its
  original source path/hash remain in the lock record; the Matt Pocock MIT
  notice applies.
- `setup`, `specify`, and `breakdown-tickets` rename Matt Pocock's
  `setup-matt-pocock-skills`, `to-spec`, and `to-tickets`. Original upstream
  source paths and hashes are retained; the Matt Pocock MIT notice applies.
  Their local workflows now enforce supported-provider setup and the
  Discovery-artifact to requirements to approved-ticket sequence.
- `discovery` retains Matt Pocock's `wayfinder` provenance and absorbs useful
  inquiry, alternative-comparison, visual-question, and artifact-checking
  guidance from Jesse Vincent's `brainstorming`. The separate Superpowers
  workflow, reviewer template, and browser runtime are retired. Both MIT
  notices apply alongside the repository license for new Scout integration.
  The human-authored Discovery intent and Scout doctrine remain unchanged.
- `interrogate` combines `grilling`, `grill-me`, and `grill-with-docs` from
  Matt Pocock's collection. Its primary lock record retains the imported
  `skills/productivity/grilling/SKILL.md` source.
- `patch` combines the previously consolidated `debug` with Julius Brussee's
  `surgical-patch`: Matt Pocock's `diagnosing-bugs`, Jesse Vincent's
  `systematic-debugging` from Superpowers, and Julius Brussee's `investigate-first`
  remain part of its foundations.
  Its primary lock record retains `skills/engineering/diagnosing-bugs/SKILL.md`.
  The supporting tracing, waiting, validation, test-pollution, and evaluation
  material comes from Superpowers; the human-assisted loop template comes from
  Matt Pocock's collection. All three MIT notices above apply.
- `roast` combines Matt Pocock's `code-review`, Jesse Vincent's
  `requesting-code-review` and reviewer template, and Julius Brussee's
  `caveman-review`. Its primary lock record retains
  `skills/engineering/code-review/SKILL.md`. The new any-material workflow,
  code heuristics, reviewer contract, and optional terse output implement this
  repository's original Roast intent, copied unchanged from the archive.
  All three MIT notices above apply alongside the repository license for new
  material. The old atomic review machinery is not restored.
- `tdd` combines Matt Pocock's `tdd` with Jesse Vincent's
  `test-driven-development` from Superpowers, including the adapted
  `writing-good-tests.md` reference. Its primary lock record retains
  `skills/engineering/tdd/SKILL.md`. Both MIT notices apply.
- `verify` combines Julius Brussee's `verify-and-stop` with Jesse Vincent's
  `verification-before-completion` from Superpowers. Its primary lock record
  retains `skills/verify-and-stop/SKILL.md`. Both MIT notices apply.
- `ship` combines Matt Pocock's `implement` and `implement-spec` with Julius
  Brussee's `lean-build`, adapted into a delivery coordinator.
  Its primary lock record retains `skills/engineering/implement/SKILL.md`.
  It also retains adapted worker/report guidance from Jesse Vincent's
  `subagent-driven-development`; Roast retains that package's scoped
  re-review guidance. The alternate executor and its runtime scripts are
  retired. All three MIT notices apply.
- `joe-mode` combines Matt Pocock's `ask-matt` routing and phase-boundary
  guidance with Jesse Vincent's `using-superpowers` skill-selection discipline.
  Its primary lock record retains `skills/engineering/ask-matt/SKILL.md`.
  Both MIT notices apply. The agreed local intent drives a new concurrent,
  anchored orchestration workflow; Copilot runtime guidance replaces the
  upstream router's other-harness tool mappings. Setup, readiness publishing,
  and delivery references are adapted for GitHub/Azure DevOps coordination.

These workflows and their callers have been adapted locally. Original import
records and contents remain recoverable from Git history.

The shared [commit-message policy](./COMMIT-STYLE.md) adapts Julius
Brussee's `caveman-commit`. The standalone skill is retired; its policy is a
library default rather than a routable import. The Caveman MIT notice above
continues to apply alongside the repository license for new material.
Preserve applicable attribution and license when distributing a policy copy.

`scout` renames Julius Brussee's `caveman-explore`, retaining its read-only
repository-localization behavior, tests, and original import source/hash.
The Caveman MIT notice above applies. This skill is distinct from the
repository's human-authored Scout doctrine.

`doctrine` is a locally authored read-only catalog/selection/loading skill,
not an installer import. It contains the unchanged existing human-curated
doctrine sources and uses the repository MIT license. The newly requested
`worktrees` doctrine and Ship's workspace procedure adapt Jesse Vincent's
`using-git-worktrees`, whose standalone skill is retired. The Superpowers MIT
notice applies to that adapted material alongside the repository license.

`eli5` is restored locally from this repository's unchanged archived intent
and adapted subject-grounding/explanation guidance. It uses the repository MIT
license, has no upstream installer record, and does not restore the archived
atomic framework, recording, or structural-checker machinery.

`status-report` is restored locally from its unchanged archived intent, with
a new single-snapshot workflow and Joe-mode's approved event callers. It uses
the repository MIT license, has no upstream installer record, and does not
restore archived recording or orchestration machinery.

`changelog` is a new local integration workflow, not an installer import or
a restoration of the archive's proposal-only changelog package. It references
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) by Olivier Lacan
for its human-facing conventions; that project's sources are not vendored.
The new workflow uses the repository MIT license.

`poc` adapts Matt Pocock's `prototype`, including its logic and UI references,
and retains the original `skills/engineering/prototype/SKILL.md` import record.
Its broader experiment workflow uses this repository's unchanged archived
proof-of-concept intent. `discovery` retains the `wayfinder` import provenance
while its workflow is rebuilt around this repository's unchanged discovery
intent. The research workflow and affected routing/tracker references are
adapted to return evidence without automatic repository or tracker writes.
Matt Pocock's MIT notice applies alongside the repository MIT license for new
material. No archived runtime or atomic composition is restored.

`shepherd` is locally authored from this repository's retained human intent,
not an upstream import. It uses the repository MIT license and has no installer
lock record. The active Ship and Shepherd intents contain human-approved
updates; their historical originals remain in the archive.

The locally authored `synthesize` workflow and its intent also draw on Julius Brussee's
`caveman-compress` for token-focused prose compression and exact technical
content preservation. The Caveman MIT notice above applies alongside the
repository MIT license for new material. This is a new implementation of the
retained intent, not an installer import; it has no lock record and does not
restore Caveman's compression runtime.

## Archived collection

Earlier third-party adaptations and their notices remain in
[`archive/atomic-v1/NOTICE.md`](https://github.com/jdylanmc/agent-skills/blob/main/archive/atomic-v1/NOTICE.md) and the archived
files themselves. Paths in those historical documents describe the old layout.
