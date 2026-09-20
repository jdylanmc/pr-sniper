# Commit messages

Default: terse, exact Conventional Commits. Intent over narration; explain why
when the diff cannot. This is a formatting policy, not permission to stage,
commit, amend, push or rewrite history.

## Subject

- Use `<type>(<scope>): <imperative summary>`; scope is optional.
- Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `build`,
  `ci`, `style`, `revert`.
- Prefer 50 characters or fewer; the complete subject has a 72-character cap.
- Use imperative verbs, match project capitalization and omit a trailing period.
- Mark breaking changes with the Conventional Commits marker and explain the
  break in the body.

## Body and references

Omit the body when the subject fully explains a routine change. Include it
for a non-obvious reason, consequences or necessary issue context.

Always include a meaningful body for breaking changes, security fixes, data
migrations and reverts. Explain impact, migration/recovery action or the
reason for reverting. Terseness must not hide risk or necessary instructions.

Wrap ordinary prose at 72 characters and use `-` for bullets. Preserve exact
identifiers, URLs and structured trailer values rather than breaking them.

Use GitHub references, qualifying references to other repositories. Use
closing semantics only for fully satisfied work when the delivery workflow
permits it; otherwise use a non-closing reference. Do not invent issue links.

## No filler

Avoid "this commit does," "as requested," first-person narration, redundant
filenames and restating the diff. No decorative emoji, promotional text or
generated-by boilerplate.

Preserve required authorship, sign-off, issue and other trailers exactly as
the operator and repository require. Do not invent contributors or suppress
a required trailer for brevity. Structured trailer values are not prose.

## Scope and precedence

Explicit operator instructions and repository commit conventions take
precedence. Surface material conflicts. Keep existing/replayed commits
unchanged unless history or message rewriting is separately authorized.

This policy is independent of chat style. Other documents, code comments,
PR descriptions and messages keep their own writing rules. When asked only
to draft a message, return a paste-ready block without staging or committing.

## Attribution and license

Adapted from the installed agent-skills commit policy and Julius Brussee's
MIT-licensed `caveman-commit` skill. This policy contains no Caveman
Engine-linked runtime material.

MIT License

Copyright (c) 2026 Dylan McCurry
Copyright (c) 2026 Julius Brussee

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
