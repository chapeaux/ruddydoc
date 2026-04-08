# RuddyDoc Team — Agent Skill Definitions

This directory contains skill definitions for the team of Claude Code agents that implement RuddyDoc. Each subdirectory represents a role with a `SKILL.md` file defining responsibilities, handoff protocols, standards, and pitfalls.

## Roles

| Role | Directory | Responsibility |
|------|-----------|----------------|
| **Team Lead** | `team-lead/` | Orchestrates all work, assigns tasks, enforces validation chains |
| **Ontologist** | `ontologist/` | RDF, SPARQL, SHACL, vocabulary curation, Semantic Copilot design |
| **Architect** | `architect/` | System design, crate boundaries, public APIs, dependency policy |
| **Rust Engineer** | `rust-engineer/` | Core Rust implementation across all crates |
| **Deno Engineer** | `deno-engineer/` | Deno plugin bridge, TypeScript SDK, JSON-RPC protocol |
| **Frontend Engineer** | `frontend-engineer/` | Web components, templates, HTML output, authoring UI |
| **Designer** | `designer/` | CLI UX, authoring UI design, error messages, accessibility design |
| **QA Engineer** | `qa-engineer/` | Functional testing, performance, accessibility, UX validation |
| **Legal** | `legal/` | Licensing, attribution, dependency audit, IP compliance |
| **Compliance** | `compliance/` | W3C spec conformance, structured data validation, HTML compliance |
| **DevOps** | `devops/` | CI/CD, release pipeline, cross-compilation, distribution |

## Handoff Flow

Every piece of work flows through this general pattern:

```
Team Lead assigns work
       ↓
Engineer implements (Rust / Deno / Frontend)
       ↓
Domain expert validates (Ontologist / Compliance / Designer)
       ↓
QA Engineer tests (functional / performance / accessibility)
       ↓
Team Lead accepts
```

The specific validation chain depends on what changed — see the Team Lead's `SKILL.md` for the full matrix.

## Core Principle

All roles share one non-negotiable principle:

> **Users should never need to know RDF.** Every user-facing interface must use human-readable language. IRIs, prefixes, and RDF jargon are internal implementation details.
