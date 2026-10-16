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
