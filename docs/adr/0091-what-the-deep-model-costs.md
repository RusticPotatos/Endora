# 0091 — What the deep model costs

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not deployed when written.

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** The deep model and what it cost are retired. The reasoning below is kept; none of its mechanism runs.

## Context

The deep model is somebody else's paid API, and Endora now uses it in three ways: the Ask deep
button, the automatic re-check of an answer that was not looked up (ADR 0087), and the daily brief.
The person had no way to see what that spends or how much credit was left, and a loop that called
it too often would have been invisible until the provider's bill.

## Decision

Two things, kept apart because they answer different questions.

- **The provider's own balance, when it has one.** DeepSeek publishes `GET /user/balance`
  (USD or CNY; granted and topped-up amounts; whether the account can still be called). For that
  host Endora asks it, with the key it already holds, and shows the figure and a warning under
  one dollar. Any other provider has no such call that Endora knows of, so it says nothing
  about a balance rather than guessing. The request is made when the person opens a screen
  that shows it (Settings → Models, or the inspector), reused for 30 seconds, never on a timer.
- **What Endora's own calls used.** Every call to the deep model records the token counts the
  provider reports (`usage`, in either of the two shapes used for cached input) in a
  `model_usage` table. The console shows one line: this month's total (in dollars once priced, otherwise tokens). Individual calls are stored but not listed. A dollar
  figure appears only after the person enters what a million tokens cost, because Endora does
  not know any provider's prices; cached input can have its own price.

Recording is best-effort and never fails the turn it describes. Only the deep model's calls are
recorded: the local model costs nothing to meter.

## Consequences and limits

- The balance is the provider's, so it also counts use from anywhere else the key is used; the
  usage log counts only what this node sent.
- A provider that does not return `usage` yields no rows, and the screen shows nothing used.
- Dollar estimates are only as good as the prices entered. Token counts are the provider's.
- The balance call has been checked against the provider's published shape and a stubbed
  response, not against a live account.
