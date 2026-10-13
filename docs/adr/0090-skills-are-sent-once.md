# 0090 — Skills are sent once

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not deployed when written.

## Context

The context inspector (ADR 0088) showed the skill list twice: once as a plain list in the
instructions ("set \"use\" to one of these ids") and once as native tool definitions. The text
list belongs to the older JSON-envelope path; when tools are sent natively the model is already
given each skill's name, description and input schema, so the list repeated them.

## Decision

When tool definitions are offered, the instructions do not also list the skills. The plain list
remains for the path with no tools (and is shown in the inspector only then). The Ask deep
planner and the automatic re-check use the same request builder, so they get the saving too.

## Measured

`tool_choice_eval` (ignored, live), `qwen2.5:7b`, 3 repeats per question at temperature 0.5,
same catalogue before and after:

| | full catalogue | shortlisted |
|---|---|---|
| Before | 13 / 24 | 14 / 24 |
| After | 12 / 24 | 13 / 24 |

A difference of one trial in 24 is inside the run-to-run noise at this sample size, so the change
is not shown to help or hurt tool choice; what it does is remove text the model had already been
given. The saving is the size of the plain list, which grows with the number of skills; the
inspector shows the real figure on a node.

## Limits

Removing tools the person does not use (the larger lever, and their setting) is separate.
