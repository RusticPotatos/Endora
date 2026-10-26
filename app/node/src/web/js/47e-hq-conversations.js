// Information-only peer conversations (ADR 0137). Keep one bounded page in memory.
// Nothing to set up (ADR 0139): a conversation starts itself when one member asks another.
// The list loads itself and stays fresh (`loadedAt`); the owner's off switch waits under `manage`.
const HQ_CHAT = { directory: null, page: null, assistance: null, operations: null, id: null, after: 0, busy: false, error: "", manage: false, loadedAt: 0 };
const HQ_CHAT_FRESH_MS = 30000;

// A quiet background refresh of the first page: no busy state, and no redraw of its own; HQ's
// three-second redraw shows it, and holds off while someone types.
async function hqConversationsRefresh() {
  HQ_CHAT.loadedAt = Date.now();
  try {
    HQ_CHAT.directory = await api("GET", "/v1/hq/conversations?after=0");
  } catch (_) { /* the next refresh tries again */ }
}

function hqConversations(s) {
  if (!HQ_CHAT.busy && Date.now() - HQ_CHAT.loadedAt > HQ_CHAT_FRESH_MS) setTimeout(hqConversationsRefresh, 0);
  const d = HQ_CHAT.directory;
  const name = (id) => esc(((s.members || []).find((m) => m.id === id) || {}).name || `Member ${id}`);
  const button = (what, id, label, disabled = false) => `<button class="hq-btn tall" data-act="hq:conversations:${what}:${id}"${disabled || HQ_CHAT.busy ? " disabled" : ""}>${label}</button>`;
  let body = '<p class="hq-note">Loading conversations…</p>';
  if (d) {
    // The latest activity first; one with nothing said yet goes last, newest first.
    const latest = [...(d.conversations || [])].sort((x, y) => (y.latest || 0) - (x.latest || 0) || (y.at || 0) - (x.at || 0));
    const when = (c) => c.latest ? `last ${esc(new Date(c.latest).toLocaleString())}` : "nothing said yet";
    body = `<p class="hq-note">What the agents said to each other. Anyone may ask anyone; a conversation starts when one does, and is kept until you delete it.</p>
      <ul class="hq-lines">${latest.map((c) => hqLine(`hq:conversations:open:${c.id}`, c.participants.map(name).join(" ↔ "), `${Number(c.messages)} messages · ${when(c)}`)).join("") || `<li class="hq-empty">${d.enabled ? "Nobody has asked anyone yet." : "Messaging is off: Manage turns it on."}</li>`}</ul>
      <div class="hq-row">${d.has_more ? button("load", d.after, "Next conversations") : ""}${button("manage", "toggle", HQ_CHAT.manage ? "Done managing" : "Manage")}</div>`;
    if (HQ_CHAT.manage) {
      body += `<section class="hq-card"><h3>Manage</h3><p class="hq-note">Messaging is ${d.enabled ? "on" : "off"}. There is nothing to set up: when one member asks another, their conversation starts itself. Turning it off stops every conversation and open request.</p>
        <div class="hq-row">${button("enable", d.enabled ? "off" : "on", d.enabled ? "Turn messaging off" : "Turn messaging on")}${button("load", "first", "Refresh list")}${button("operations", "first", "Operation history")}</div></section>`;
    }
  }
  if (HQ_CHAT.page) {
    const page = HQ_CHAT.page;
    body += `<section class="hq-card"><h3>Conversation ${Number(HQ_CHAT.id)}</h3>
      <p class="hq-note">This is one page of retained information, in recorded order. Statements are claims; references do not grant permissions.</p>
      <ul class="hq-thread">${(page.messages || []).slice(0, 50).map((m) => `<li><b>${esc(((s.members || []).find((p) => p.id === m.member) || {}).name || `Member ${m.member}`)}</b> <small>message ${Number(m.id)}, run ${Number(m.run)}${m.at ? ` · ${esc(new Date(m.at).toLocaleString())}` : ""}</small><p style="white-space:pre-wrap;overflow-wrap:anywhere">${esc(m.text)}</p>${m.evidence.length ? `<small>Original references: ${m.evidence.map(esc).join(", ")}</small>` : ""}</li>`).join("") || '<li class="hq-empty">No messages on this page.</li>'}</ul>
      <div class="hq-row">${button("open", HQ_CHAT.id, "First page")}${button("refresh", HQ_CHAT.id, "Refresh page")}${button("save", HQ_CHAT.id, "Save this page")}${button("assistance", HQ_CHAT.id, "Assistance requests")}
      ${page.has_more ? button("next", HQ_CHAT.id, "Next page") : ""}${button("delete", HQ_CHAT.id, "Delete conversation")}</div></section>`;
    if (HQ_CHAT.assistance) {
      const a = HQ_CHAT.assistance;
      const requestRows = (a.requests || []).map((r) => {
        const requester = ((s.members || []).find((m) => m.id === r.requester) || {}).name || `Member ${r.requester}`;
        const expert = ((s.members || []).find((m) => m.id === r.expert) || {}).name || `Member ${r.expert}`;
        const spend = ((Number(r.spent || 0) + Number(r.notice_spent || 0)) / 1000000).toFixed(4);
        const delivery = r.notice_cancelled ? "Delivery cancelled" : r.notified ? `Result turn ${esc(r.notice_status || "queued")}` : "Waiting to notify requester";
        return `<li>Request ${Number(r.id)} · ${esc(r.status)} · ${esc(requester)} to ${esc(expert)}
          <p>${esc(r.objective)}</p>${r.reason ? `<p>${esc(r.reason)}</p>` : ""}
          <small>Requester turn: ${r.requester_run ? replayRunLink(r.requester_run, `R${Number(r.requester_run)}`) : "unavailable"} · expert turn: ${r.expert_run ? replayRunLink(r.expert_run, `R${Number(r.expert_run)}`) : "unavailable"}</small>
          <small>Project ${Number(r.project)} · ${Number(r.attempts || 0)} attempts · exchange spend $${spend} · deadline ${esc(new Date(r.deadline_at).toLocaleString())}${r.status === "processing" && r.run_status === "retry_waiting" && r.retry_at ? ` · next attempt ${esc(new Date(r.retry_at).toLocaleString())}` : ""}</small>
          ${(r.evidence || []).length ? `<p>Original source locators: ${r.evidence.map(esc).join(", ")}</p>` : ""}
          ${r.answer ? `<p>Answer: ${esc(r.answer)}</p><small>Answer sources: ${(r.answer_evidence || []).map(esc).join(", ") || "none cited"} · ${delivery}</small>` : ""}
          ${r.finding ? `<p>Requester finding: ${esc(r.finding.text)}</p><small>Attributed claim · ${replayRunLink(r.finding.run, `result run ${Number(r.finding.run)}`)} · original sources: ${(r.finding.evidence || []).map(esc).join(", ") || "none cited"}</small>` : '<p class="hq-note">No retained requester finding recorded.</p>'}
          ${["queued", "processing"].includes(r.status) ? button("cancel-request", r.id, "Cancel request") : ""}</li>`;
      }).join("");
      body += `<section class="hq-card" style="overflow-wrap:anywhere"><h3>Assistance in conversation ${Number(HQ_CHAT.id)}</h3>
        <p class="hq-note">Each request has a fifteen-minute deadline. Answers are attributed peer testimony. A source grant lasts 24 hours, applies only to this conversation, and gives no task authority.</p>
        <ul class="hq-thread">${requestRows || '<li class="hq-empty">No assistance requests on this page.</li>'}</ul>
        <div class="hq-row">${button("assistance", HQ_CHAT.id, "First requests")}${button("save-assistance", HQ_CHAT.id, "Save this request page")}${a.has_more ? button("assistance-next", HQ_CHAT.id, "Next requests") : ""}</div>
        <ul class="hq-thread">${(a.grants || []).map((g) => `<li>${esc(g.source)} · expires ${esc(new Date(g.expires_at).toLocaleString())} ${button("revoke-source", esc(g.source), "Revoke access")}</li>`).join("") || '<li class="hq-empty">No active source grants.</li>'}</ul>
        <div class="hq-row"><label>Original source <input id="hq-assistance-source" placeholder="d12 or r8" maxlength="24"></label>${button("grant-source", HQ_CHAT.id, "Grant access")}</div></section>`;
    }
  }
  if (HQ_CHAT.operations) {
    const history = HQ_CHAT.operations;
    body += `<section class="hq-card"><h3>Operation history</h3><p class="hq-note">Stored and replayed sends are recorded separately. A successful read means a page was returned; it does not mean an agent acted on it.</p>
      <ul class="hq-thread">${(history.operations || []).slice(0, 50).map((o) => `<li>Member ${Number(o.member)}, run ${Number(o.run)}${o.conversation ? `, conversation ${Number(o.conversation)}` : ""}: ${esc(o.operation)} — ${esc(o.reason)} · ${esc(new Date(o.at).toLocaleString())}</li>`).join("") || '<li class="hq-empty">No operations on this page.</li>'}</ul>
      <div class="hq-row">${button("operations", "first", "First operations")}${history.has_more ? button("operations", history.after, "Next operations") : ""}</div></section>`;
  }
  return `<section class="hq-card"><h2>Conversations</h2>${HQ_CHAT.error ? `<p role="alert">${esc(HQ_CHAT.error)}</p>` : ""}${body}</section>`;
}

async function hqConversationAct(what, id) {
  if (HQ_CHAT.busy) return true;
  if (what === "manage") { HQ_CHAT.manage = !HQ_CHAT.manage; render(); return true; }
  if (what === "delete" && !confirm("Delete this conversation and all its messages?")) return true;
  if (what === "save" || what === "save-assistance") {
    const page = what === "save-assistance" ? HQ_CHAT.assistance : HQ_CHAT.page;
    if (!page) return true;
    const url = URL.createObjectURL(new Blob([JSON.stringify(page, null, 2)], { type: "application/json" }));
    const a = document.createElement("a"); a.href = url; a.download = `conversation-${Number(HQ_CHAT.id)}-${what}-after-${Number(page.after)}.json`; a.click(); URL.revokeObjectURL(url);
    return true;
  }
  HQ_CHAT.busy = true; HQ_CHAT.error = "";
  try {
    if (what === "enable") {
      await api("POST", "/v1/hq/conversations/enabled", { enabled: id === "on" });
      HQ_CHAT.directory = await api("GET", "/v1/hq/conversations?after=0");
    } else if (what === "load") {
      HQ_CHAT.directory = await api("GET", `/v1/hq/conversations?after=${id === "first" ? 0 : Number(id)}`);
      HQ_CHAT.loadedAt = Date.now();
    } else if (what === "operations") {
      HQ_CHAT.operations = await api("GET", `/v1/hq/conversations/operations?after=${id === "first" ? 0 : Number(id)}`);
    } else if (["open", "next", "refresh"].includes(what)) {
      const after = what === "next" ? Number((HQ_CHAT.page || {}).after) || 0 : what === "refresh" ? HQ_CHAT.after : 0;
      HQ_CHAT.page = null;
      HQ_CHAT.id = Number(id); HQ_CHAT.after = after;
      HQ_CHAT.page = await api("GET", `/v1/hq/conversations/${Number(id)}?after=${after}`);
      // Opening one shows what was asked and answered in it too: one tap to watch the exchange.
      if (what === "open") HQ_CHAT.assistance = await api("GET", `/v1/hq/conversations/${Number(id)}/assistance?after=0`);
    } else if (what === "assistance" || what === "assistance-next") {
      const after = what === "assistance-next" ? Number((HQ_CHAT.assistance || {}).after) || 0 : 0;
      HQ_CHAT.assistance = await api("GET", `/v1/hq/conversations/${Number(id)}/assistance?after=${after}`);
    } else if (what === "cancel-request") {
      await api("POST", `/v1/hq/conversations/${Number(HQ_CHAT.id)}/assistance/${Number(id)}/cancel`, {});
      HQ_CHAT.assistance = await api("GET", `/v1/hq/conversations/${Number(HQ_CHAT.id)}/assistance?after=0`);
    } else if (what === "grant-source" || what === "revoke-source") {
      const source = what === "grant-source" ? val("hq-assistance-source").trim() : String(id);
      if (!/^[dr][1-9][0-9]*$/.test(source)) throw new Error("Enter an original document or receipt locator, such as d12 or r8.");
      if (what === "revoke-source") await api("POST", `/v1/hq/conversations/${Number(HQ_CHAT.id)}/sources/revoke`, { source });
      else await api("POST", `/v1/hq/conversations/${Number(HQ_CHAT.id)}/sources`, { source });
      HQ_CHAT.assistance = await api("GET", `/v1/hq/conversations/${Number(HQ_CHAT.id)}/assistance?after=0`);
    } else if (what === "delete") {
      await api("POST", `/v1/hq/conversations/${Number(id)}/delete`, {});
      HQ_CHAT.page = null; HQ_CHAT.assistance = null; HQ_CHAT.id = null;
      HQ_CHAT.operations = null;
      HQ_CHAT.directory = await api("GET", "/v1/hq/conversations?after=0");
    }
  } catch (e) { HQ_CHAT.error = String(e.message || e); }
  finally { HQ_CHAT.busy = false; render(); }
  return true;
}

// The help a task's own turns asked colleagues for, shown on the task where its work is read.
// One task's first page is kept, and refreshed in the background while the task stays open.
const HQ_TASK_HELP = { task: null, page: null, loadedAt: 0 };

async function hqTaskHelpRefresh(task) {
  HQ_TASK_HELP.loadedAt = Date.now();
  try {
    const page = await api("GET", `/v1/hq/tasks/${Number(task)}/assistance?after=0`);
    if (HQ_TASK_HELP.task === task) HQ_TASK_HELP.page = page;
  } catch (_) { /* the next refresh tries again */ }
}

function hqTaskHelp(task) {
  task = Number(task);
  if (!Number.isSafeInteger(task) || task <= 0) return "";
  if (HQ_TASK_HELP.task !== task) Object.assign(HQ_TASK_HELP, { task, page: null, loadedAt: 0 });
  if (Date.now() - HQ_TASK_HELP.loadedAt > HQ_CHAT_FRESH_MS) setTimeout(() => hqTaskHelpRefresh(task), 0);
  const requests = (HQ_TASK_HELP.page && HQ_TASK_HELP.page.requests) || [];
  if (!requests.length) return "";
  const said = { queued: "waiting for an answer", processing: "being answered", declined: "declined", cancelled: "cancelled", expired: "no answer in time", failed: "failed" };
  const rows = requests.map((r) => {
    const asker = esc(hqName(r.requester)), expert = esc(hqName(r.expert));
    const outcome = r.answer
      ? `<p><b>${expert}</b>: ${esc(r.answer)}</p>${r.finding ? `<p><b>${asker}</b> concluded: ${esc(r.finding.text)}</p>` : ""}`
      : `<p class="hq-note">${esc(said[r.status] || r.status)}${r.reason ? `: ${esc(r.reason)}` : ""}</p>`;
    return `<li><b>${asker}</b> asked <b>${expert}</b>: ${esc(r.objective)}${outcome}
      <button class="hq-link" data-act="hq:conversations:open:${Number(r.conversation)}">Their conversation</button></li>`;
  }).join("");
  return `<h3>Help on this task</h3><ul class="hq-thread" style="overflow-wrap:anywhere">${rows}</ul>${HQ_TASK_HELP.page.has_more ? '<p class="hq-note">The first 50 exchanges; their conversations hold the rest.</p>' : ""}`;
}

// Older task questions stay read-only. Keep one page and at most 64 back cursors.
const HQ_PAST = { task: null, page: null, after: 0, previous: [], busy: false, error: "" };

function hqPastQuestions(task) {
  task = Number(task);
  if (!Number.isSafeInteger(task) || task <= 0) return "";
  const same = HQ_PAST.task === task, page = same && HQ_PAST.page;
  const button = (what, label, disabled = false) => `<button class="hq-btn tall" data-act="hq:past-questions:${task}:${what}"${disabled || HQ_PAST.busy ? " disabled" : ""}>${label}</button>`;
  const run = (id, label) => Number.isSafeInteger(id) && id > 0 ? replayRunLink(id, label) : "";
  const outcomes = { queued: "Waiting when recorded", answered: "Answered", from_record: "Answer from the record", cancelled: "Cancelled" };
  let body = button("first", "Load saved questions");
  if (page) {
    const records = (page.records || []).slice(0, 50).map((row) => {
      const answer = row.answer;
      const said = answer ? `<p><b>${esc(answer.author)}</b>${answer.unsupported ? ' <span class="hq-unsupported">unsupported</span>' : ""}</p><p style="white-space:pre-wrap;overflow-wrap:anywhere">${esc(answer.text)}</p>${(answer.evidence || []).length ? `<small>Saved references: ${answer.evidence.map(esc).join(", ")}</small>` : ""}`
        : '<p class="hq-note">Saved answer unavailable.</p>';
      return `<li style="padding:12px 0;border-top:1px solid var(--hq-line)"><b>${esc(hqName(Number(row.requester)))} asked ${esc(hqName(Number(row.expert)))}</b>
        <small style="display:block">${esc(outcomes[row.outcome] || row.outcome || "Outcome unavailable")}${row.at ? `; ${esc(new Date(row.at).toLocaleString())}` : ""}; ${row.project == null ? "Project not recorded" : `Project ${Number(row.project)}`}</small>
        <p style="white-space:pre-wrap;overflow-wrap:anywhere">${esc(row.question)}</p>${said}
        ${row.reason ? `<p class="hq-note" style="overflow-wrap:anywhere">${esc(row.reason)}</p>` : ""}
        <div class="hq-row">${run(row.requester_run, "Asking turn")}${run(row.expert_run, "Answering turn")}</div></li>`;
    }).join("");
    body = `<p class="hq-note">Saved in recorded order. Missing answers are marked unavailable.</p><ul class="hq-thread">${records || '<li class="hq-empty">No saved questions on this page.</li>'}</ul>
      <div class="hq-row">${button("first", "First page")}${button("previous", "Previous page", !HQ_PAST.previous.length)}${button("next", "Next page", !page.has_more)}${button("refresh", "Refresh page")}${button("save", "Save page")}</div>`;
  }
  return `<section class="hq-card"><h3>Past colleague questions</h3>${same && HQ_PAST.busy ? '<p class="hq-note">Loading saved questions...</p>' : ""}${same && HQ_PAST.error ? `<p class="hq-note" role="alert">${esc(HQ_PAST.error)}</p>` : ""}${body}</section>`;
}

async function hqPastQuestionsAct(rawTask, what) {
  const task = Number(rawTask);
  if (!Number.isSafeInteger(task) || task <= 0 || !["first", "previous", "next", "refresh", "save"].includes(what)) return false;
  if (HQ_PAST.busy) return true;
  const same = HQ_PAST.task === task;
  if (what === "save") {
    if (!same || !HQ_PAST.page) return true;
    const url = URL.createObjectURL(new Blob([JSON.stringify(HQ_PAST.page, null, 2)], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url; link.download = `task-${task}-questions-after-${HQ_PAST.after}.json`; link.click(); URL.revokeObjectURL(url);
    return true;
  }
  let after = same ? HQ_PAST.after : 0;
  let previous = same ? HQ_PAST.previous.slice(-64) : [];
  if (what === "first" || !same) { after = 0; previous = []; }
  if (what === "next") {
    if (!same || !HQ_PAST.page || !HQ_PAST.page.has_more) return true;
    previous.push(after); previous = previous.slice(-64); after = Number(HQ_PAST.page.after);
  }
  if (what === "previous") {
    if (!previous.length) return true;
    after = previous.pop();
  }
  if (!Number.isSafeInteger(after) || after < 0) return true;
  if (!same) { HQ_PAST.task = task; HQ_PAST.page = null; HQ_PAST.previous = []; HQ_PAST.after = 0; }
  HQ_PAST.busy = true; HQ_PAST.error = ""; render();
  try {
    const page = await api("GET", `/v1/hq/tasks/${task}/consultations?after=${after}`);
    if (!Array.isArray(page.records)) throw new Error("Saved history is unavailable.");
    HQ_PAST.page = { ...page, records: page.records.slice(0, 50) };
    HQ_PAST.after = after; HQ_PAST.previous = previous;
  } catch (error) { HQ_PAST.error = String(error.message || error); }
  finally { HQ_PAST.busy = false; render(); }
  return true;
}
