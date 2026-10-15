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

## Amendment: the reads are also available to chat

The handheld's entries in the skills list were settings-only so that nothing here reached the
model. That was right for the console's operations and wrong for plain reading: a service the
person had already set up could not be asked about in chat. Five read-only skills now borrow the
settings of the entries they speak for, so nothing is configured twice and each is offered only
once its entry is set up: the Docker and Portainer container lists, the Sonarr and Radarr queues,
and the NAS figures.

What does not change. The entries themselves stay settings-only. Mutations (container controls,
media requests, jobs) stay on the explicit confirmed path and are not available to chat. The
skills take no input, so the model supplies no address, path or parameter; they use the same
bounded fetch as the Overview page; they return names, states, titles, progress and figures,
never ids, labels, environment or logs. Container logs and media search remain handheld-only,
as do Overseerr and the home screen.

Privacy. These results describe the person's own infrastructure, so they are marked as outside
material like mail is (ADR 0064): the turn that read them can answer but not act, and it is never
escalated to the deep model, derived from the marks each time (ADR 0070). For the queues that is
also simply correct, since their titles are release names written by indexers. Disguising names
on the way out was considered and set aside: the escalation door disguises the person's messages
and not what a skill returned, so a tool result carrying a real name reaches the deep model
unchanged. That is true of every skill and is a separate defect, recorded here so it is not lost.
