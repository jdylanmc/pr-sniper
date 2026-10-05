---
name: laziness
description: "Disciplined economy for high-impact changes with low maintenance burden."
scope: shared-engineering-doctrine
---

# Laziness Doctrine

## Prime directive

Keep it simple. Keep it minimal. Don't add things you won't need, and don't modify things you don't need to.

## Position

Code is cheap for an agent to write and expensive for a human to inherit. Seek the most useful complete result with the least code, indirection, and coordination burden that can safely deliver it. Think like a tired maintainer.

Laziness means refusing unnecessary work. It never means skipping investigation, correctness, validation, security, accessibility, compatibility, diagnostics, or proof.

## Rules

- **Delete before adding.** First look for redundant behavior, obsolete paths, purposeless pass-throughs, duplicated choices, and representation leaks that evidence shows can be removed.
- **Make the smallest complete change.** Solve the root problem and every directly affected contract, error path, test, migration, documentation surface, and safeguard. Authorized scope always wins over cleanup opportunity. Report unrelated cleanup separately.
- **Keep the cognitive path flat.** Maintainers should find ownership and follow behavior without crossing layers that only forward, rename, or translate. More than three files or layers is a warning to inspect for accidental indirection, not an automatic violation. A rich interface that hides substantial work is not inherently a deep call chain.
- **Question long data paths.** When a signal crosses several functions, types, schemas, pipelines, or services, compare that design with a direct route. Preserve ownership, validation, type safety, authorization, observability, consistency, lifecycle semantics, and compatibility.
- **Make structure earn its cost.** Keep a layer, abstraction, wrapper, schema, or generalized mechanism when it enforces an invariant, owns a responsibility, isolates change, stabilizes a contract, exposes failure, preserves compatibility, or improves safe testing. Speculative flexibility is not evidence.
- **Optimize maintenance, not line count.** Smaller diffs and fewer lines are useful evidence, never the definition of simplicity.

## Boundaries

Do not flatten architecture by mixing responsibilities, bypassing trust boundaries, weakening types, introducing hidden coupling, or deleting safeguards. Existing adjacent leaks stay outside scope unless the authorized result requires their removal. Generated or framework-mandated structure may be necessary.
