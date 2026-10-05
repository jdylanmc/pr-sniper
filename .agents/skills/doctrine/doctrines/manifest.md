---
schema-version: 1
doctrine:
  - id: bounded-context
    path: bounded-context.doctrine.md
    sha256: 920cfab9e0d082dcca40ace4e974dfe740f9e6daffda4100941e1ae41cf41e09
  - id: code
    path: code.doctrine.md
    sha256: fc266284da99b1525982846beb1b285ed8d63fe90315352e94853b55d5310aae
  - id: cyclomatic-complexity
    path: cyclomatic-complexity.doctrine.md
    sha256: f70c27299cc909511cbc28a0ff52f21b50099b28d739105f916b34220f6fb6be
  - id: data
    path: data.doctrine.md
    sha256: 633f1df770c55ddb633e55c644b8f5d709b4ef6068f4d242a4f185b9553ea67f
  - id: data-processing
    path: data-processing.doctrine.md
    sha256: a359a4b431f88d573ccedd3202b6773bccabc9fc722707b76dbd2a98aa9727f5
  - id: documentation
    path: documentation.doctrine.md
    sha256: 8b559801e83850b8bd8111d3985d63ca00f0ad58cdf3dd0901c409f7155c3f92
  - id: laziness
    path: laziness.doctrine.md
    sha256: 25cb513c0e30e3bac62202d509cd703ba632677346b648eca645bc55f07598de
  - id: machine
    path: machine.doctrine.md
    sha256: 44eee146aaaab5e93b4036529e617303c971cc6bd368bf6d19bf19528c467f4e
  - id: nimble
    path: nimble.doctrine.md
    sha256: 053d3e876b84147a4e6d54864a3758b2af4d653ffa3e3bb41fc39cd5a2b59bd2
  - id: pragmatic
    path: pragmatic.doctrine.md
    sha256: 817bbfe3ec83600e94429f6e646a159814a1cfe6bf83139a67f763bd94d02628
  - id: sequencing
    path: sequencing.doctrine.md
    sha256: 153c5ff96aa013352da1cf043303eedc7c92c3241c7212f24f8a6a2387f1b91d
  - id: solid
    path: solid.doctrine.md
    sha256: a7b63ba8b99a1a0c9824f56bf2924c12e33e8c748452478cd9566ee40b2d847d
  - id: tactical-strategic
    path: tactical-strategic.doctrine.md
    sha256: d988010959fb21534d7b7292aeca4d32549f5ecdffe0f0dcb8e429db228987ea
  - id: testing
    path: testing.doctrine.md
    sha256: c38f5a24bac7f14c476edf15c04d4cdd3a753f52aac0bcc1c0230178e08a08c8
---

# Doctrine Manifest

This manifest defines the canonical trusted doctrine set. Resolve doctrine
paths relative to this file, reject symlinks and path escapes, and verify each
SHA-256 digest before loading guidance.
