# 0098 — It can be rebuilt

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions 5, 6, 15–18, 23,
24 and 27, recorded in [the unification plan](../plans/unification.md)). Phase 1 of that plan
builds the first part: Endora's own nightly copies and the runbook. Prepared with AI
assistance (Claude).

## Context

The owner's question: if the system were wiped, how easy would it be to set everything up
again? Looked at honestly, the answer was "partly". Everything Endora knows, every setting and
key and its whole memory, is one SQLite file in a Docker volume. The house's backup (Backrest,
restic) does copy that volume every night, but **as a live file**: Endora writes to it
continuously, and a file copied mid-write can look whole and fail to open on the day it is
needed. The ansible-nas checkout that rebuilds the house is backed up with the home folder,
but so is the vault's password, in plain text beside the vault; the backup repository is
encrypted, with its own password kept on the same machine. The backups themselves sit on a
second disk in the same machine.

## Decision

**State is two kinds, recovered two ways.**

- **Configuration** (connections and their keys, models, skills, schedules, permissions, time
  zone) is to be *declared*: a file rendered from the ansible-nas vault and applied at every
  start, the file winning over the console for what it declares. A wiped machine is rebuilt
  from git plus the vault. (Phase 5 of the plan; until then configuration is recovered with the
  memory, below.)
- **Memory** (conversations, beliefs, the mail digest, activity) is *backed up*.

**Endora makes its own consistent copies.** At 01:30 by the person's clock, before the house's
02:00 backup, the node writes `backups/daily-YYYY-MM-DD.db` beside its database through
SQLite (`VACUUM INTO`), integrity-checks it, and on Sundays keeps the same copy as
`weekly-YYYY-MM-DD.db`. The newest seven of each are kept (the owner's choice); files it did not
name are never touched. One line in Activity says it was saved and checked; a failure is said
there once a day. The live database file is still in the backup, but there is now always a
whole copy beside it.

**The rebuild kit is backed up; its key should not be stored with it.** The ansible-nas
checkout (30 MB, with the vault encrypted) is already in the house's nightly backup. The vault
password should be kept in the owner's password manager, and as a copy encrypted under a
separate recovery passphrase the owner chooses and remembers, with the plain file excluded
from the backup: a backup holding its own key protects only as well as the backup's own
password. Excluding the plain file, and whether to rewrite the snapshots that already hold it,
the owner has left for later (2026-10-16).

**Recovery is proven, not hoped for.** A runbook ([docs/recovery.md](../recovery.md)) says how
to rebuild from nothing, and a restore drill into a scratch node runs when declared
configuration lands.

## Consequences

- A disk failure, a bad upgrade or a corrupted database is recoverable to the night before.
- **Still open: the backups are on the same machine.** Fire, theft or ransomware takes them
  with the originals. An offsite copy (a second Backrest repository, for example a cloud
  bucket) is agreed and deferred until the owner picks a provider.
- Each copy holds every credential Endora has, as the database does; they stay in the node's
  private data folder and inside the house's encrypted restic repository.

## Amendment, 2026-11-09: copies are proven by restore, for both Core stores (milestone f)

A copy that opens is not a copy that restores: `integrity_check` passes a valid but **empty**
database, or one from before a table existed. Core keeps nightly copies of **both** stores it
owns, the node's `endora.db` and HQ's `hq.db` (goals, tasks, the desk), and proves each one:

- **The proof.** `endora_persistence::prove_restore` restores the copy to a private scratch file
  in a `.scratch` folder **beside the copy, inside `backups/`** (never the system's temp folder:
  HQ's container has a read-only root with only its data folder writable, so a scratch file in
  `/tmp` failed every HQ proof, and a copy of `hq.db` should not pass through a folder every
  process on the host can list; the folder is owner-only, emptied after each proof and removed
  when empty), as a restore would, opens it read-only and passes it only if its `integrity_check` is ok, its
  tables and indexes (names and SQL) equal the live database's, and every live table that holds
  rows holds rows in the copy. The live database is opened read-only and never written.
- **Named only when proven.** The nightly writes `daily-YYYY-MM-DD.db.unproven`, proves it, and
  only then renames it `daily-…db` and makes the weekly. So a daily by its own name has been
  proven, a restart finds an unproven copy and proves it instead of copying again, and the
  house's backup never meets one that failed. A copy that fails its proof is deleted.
- **Durable before named.** The proven copy (and the weekly) is fsynced before the rename to
  `daily-…db`, and `backups/` is fsynced after, so a power cut cannot leave a proven name on
  bytes that never reached the disk.
- **Retry, then say so once.** The ten-minute loop tries again on the next tick, at most three
  attempts a night (counted per process run; a panic on the copying thread counts as the attempt it
  was); the third failure is the one Activity line (the
  node) or log line (HQ) about it. Pruning runs only after a copy has proven, so the newest
  proven copy is never removed.
- **One implementation.** The schedule, names, pruning, attempt cap, proof and the ten-minute
  loop itself (`Nightly::run`, behind the crate's `runtime` feature: tokio timers and a blocking
  thread, no network) live once in `shared/persistence`; the node and HQ each call it with their
  clock and word the result.
- **HQ depends on `shared/persistence`.** HQ's own loop and its copy are the first reason the HQ
  program takes that crate directly. Accepted: `shared/` is for what is genuinely cross-cutting,
  and a restore-proven copy of every Core store is that. HQ still builds without the node's
  contexts (ADR 0136); it does not take the team's persistence a second way.
- **`endora-node verify-backup <copy> [<live-database>]`** runs the same proof on a file, against
  `ENDORA_DB` or the live database named second (so it checks a node copy and an `hq.db` copy
  alike, with no binary of HQ's own); the full-system bundle's test runs it on a good and a
  truncated copy (a step it does not pass fails the run, ADR 0147). It never starts a server.
- **Known limits.** "The live table has rows" is read at proof time: a table that was empty when
  the copy was taken and has rows by the time of the proof reads as a failure, which the retry
  covers; the proof follows the copy at once. The attempt cap resets when the process restarts.
  `secrets.key` is not in any copy and is not proven. A daily left by a node that predates this
  proof is treated as proven for its own night.
