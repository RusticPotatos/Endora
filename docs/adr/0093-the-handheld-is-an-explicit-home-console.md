# 0093: The handheld is an explicit home console

The portable terminal needs home controls, media requests, container operations,
and Ansible jobs alongside AI chat. Its integration credentials and authority
remain on the node. A capabilities application port owns this explicit surface;
infrastructure adapters use configured origins and fixed upstream routes. These
adapters are not arbitrary model tools and do not weaken the general web egress
guard. Existing AI home capabilities retain their deterministic policy boundary.

A dashboard-only session can read aggregate overview counts. It cannot read chat,
logs, entities, jobs, or private container names, and cannot write. Full sessions
are encrypted on the device behind a local passphrase. Unknown persisted session
scopes deny access instead of inheriting a broader read permission.

Explicit mutations carry a confirmed target and durable request identifier. The
node reserves that identifier before contacting an upstream. Conflicting reuse
is refused and interrupted effects remain uncertain; automatic replay is forbidden.
The journal records request completion separately from actual asynchronous job or
media completion. The client verifies observed state and never executes model text.

Ansible runs in a private host agent over a permission-controlled Unix socket,
not inside the Pi or the model runtime. Its private catalog fixes role, inventory,
and arguments. Jobs are serialized, persisted, and marked uncertain after a crash.
The existing NAS checkout and local changes are preserved. The terminal never
submits arbitrary playbook paths, shell strings, extra variables, or credentials.

The Pi kernel currently has no suspend capability. Screen-off pocket mode and
on-demand graphical sessions preserve a lightweight terminal experience; battery
runtime and remote-access support require actual device measurements.
