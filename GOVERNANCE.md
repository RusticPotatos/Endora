# Endora Governance

> Status: foundation phase. This describes a deliberately **lightweight** model
> suited to a project that does not yet have a contributor community. It will
> grow only as real needs appear — not before.

## Principles

- Governance should be as light as possible while protecting the project's
  [Constitution](docs/constitution.md) and its users.
- Decisions are made in the open where practical, and recorded so they can be
  revisited deliberately (issues and [ADRs](docs/adr/README.md)).
- We do not create committees, boards, or formal structures before there are
  contributors to fill them.

## Roles

- **Maintainers** — the people who currently review and merge changes, cut
  branches, and steward direction. In this early stage the maintainer set is
  small (initially the project founder). Maintainers are listed via the
  repository's GitHub permissions and history; reach a maintainer through their
  GitHub profile.
- **Contributors** — anyone who opens issues or proposes changes. See
  [CONTRIBUTING.md](CONTRIBUTING.md).

As the community grows, this document will define how maintainers are added,
step down, and make decisions when they disagree.

## Decision making

- **Ordinary changes** (docs, skeleton, most implementation) — a maintainer
  reviews and merges once the standards in
  [CONTRIBUTING.md](CONTRIBUTING.md) are met.
- **Architectural changes** — require an [ADR](docs/adr/README.md) and maintainer
  agreement.

## Changes that require heightened scrutiny

Some changes carry more weight than ordinary implementation and require greater
care — more review, explicit maintainer sign-off, and (where architectural) an
ADR. These are changes to:

- the **Constitution** and other foundational documents,
- **privacy** guarantees and data handling,
- **safety** and protective mechanisms,
- the **policy / permission / consent** boundary (the deterministic boundary
  around models),
- **security-sensitive** code paths.

The intent is simple: it should always be harder to weaken a protection than to
add a feature. In particular, **the Constitution is never changed autonomously**
— only by humans, deliberately, through this process (see
[constitution §11](docs/constitution.md)).

## Amending this document

Governance changes are themselves proposed in the open and adopted by maintainer
agreement. This document is expected to become more detailed as the project and
its community mature.
