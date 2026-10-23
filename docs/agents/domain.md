# Domain Docs

How engineering skills should consume this repository's domain documentation when exploring the codebase.

## Before exploring, read these

- `CONTEXT.md` at the repository root.
- `CONTEXT-MAP.md` at the repository root, if it exists.
- Relevant decisions in `docs/adr/`.

This is a single-context repository. If these files or directories do not yet exist, proceed silently; create them only when a domain-modeling or architectural-decision workflow has a concrete need.

## Use the glossary's vocabulary

Use terms defined in `CONTEXT.md` for issue titles, proposals, hypotheses, and test names. If a needed concept is absent, either reconsider the terminology or record the gap for `/domain-modeling`.

## Flag ADR conflicts

Surface contradictions with applicable ADRs rather than silently overriding them.
