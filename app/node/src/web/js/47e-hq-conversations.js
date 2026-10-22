// Information-only peer conversations (ADR 0137). Keep one bounded page in memory.
const HQ_CHAT = { directory: null, page: null, assistance: null, operations: null, id: null, after: 0, busy: false, error: "" };

function hqConversations(s) {
  const d = HQ_CHAT.directory;
  const button = (what, id, label, disabled = false) => `<button class="hq-btn" data-act="hq:conversations:${what}:${id}"${disabled || HQ_CHAT.busy ? " disabled" : ""}>${label}</button>`;
  const members = (s.members || []).filter((m) => m.role !== "ceo" && !m.paused && !m.furloughed);
  const memberOptions = members.map((m) => `<option value="${Number(m.id)}">${esc(m.name)}</option>`).join("");
  const projects = (s.projects || []).filter((p) => p.status === "active");
  let body = button("load", "first", "Load conversations");
  if (d) {
    body = `<p class="hq-note">Information messaging is ${d.enabled ? "enabled" : "disabled"}. Sending information creates no task or agent wake. Conversations are kept until you delete them.</p>
      <div class="hq-row">${button("enable", d.enabled ? "off" : "on", d.enabled ? "Disable messaging" : "Enable messaging")}${button("load", "first", "Refresh list")}${button("operations", "first", "Operation history")}</div>
      <ul class="hq-thread">${(d.conversations || []).map((c) => `<li>Conversation ${Number(c.id)}: ${c.participants.map((id) => esc(((s.members || []).find((m) => m.id === id) || {}).name || `Member ${id}`)).join(" / ")} (${Number(c.messages)} messages) ${button("open", c.id, "Open history")}</li>`).join("") || '<li class="hq-empty">No conversations on this page.</li>'}</ul>
      ${d.has_more ? button("load", d.after, "Next conversations") : ""}`;
    if (d.enabled && members.length >= 2 && projects.length) {
      body += `<details><summary>Authorize a conversation</summary><p class="hq-note">Choose two members and the project. This permits information exchange, without granting task or tool authority.</p>
        <div class="hq-row"><label>Project <select id="hq-conversation-project">${projects.map((p) => `<option value="${Number(p.id)}">${esc(p.title)}</option>`).join("")}</select></label>
        <label>First member <select id="hq-conversation-a">${memberOptions}</select></label>
        <label>Second member <select id="hq-conversation-b">${memberOptions}</select></label>
        ${button("create", "new", "Authorize")}</div></details>`;
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
          <small>Project ${Number(r.project)} · ${Number(r.attempts || 0)} attempts · exchange spend $${spend} · deadline ${esc(new Date(r.deadline_at).toLocaleString())}${r.status === "processing" && r.run_status === "retry_waiting" && r.retry_at ? ` · next attempt ${esc(new Date(r.retry_at).toLocaleString())}` : ""}</small>
          ${(r.evidence || []).length ? `<p>Original source locators: ${r.evidence.map(esc).join(", ")}</p>` : ""}
          ${r.answer ? `<p>Answer: ${esc(r.answer)}</p><small>Answer sources: ${(r.answer_evidence || []).map(esc).join(", ") || "none cited"} · ${delivery}</small>` : ""}
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
    } else if (what === "create") {
      const project = Number(val("hq-conversation-project"));
      const participants = [Number(val("hq-conversation-a")), Number(val("hq-conversation-b"))];
      if (participants[0] === participants[1]) throw new Error("Choose two different members.");
      await api("POST", "/v1/hq/conversations", { project, participants });
      HQ_CHAT.directory = await api("GET", "/v1/hq/conversations?after=0");
    } else if (what === "load") {
      HQ_CHAT.directory = await api("GET", `/v1/hq/conversations?after=${id === "first" ? 0 : Number(id)}`);
    } else if (what === "operations") {
      HQ_CHAT.operations = await api("GET", `/v1/hq/conversations/operations?after=${id === "first" ? 0 : Number(id)}`);
    } else if (["open", "next", "refresh"].includes(what)) {
      const after = what === "next" ? Number((HQ_CHAT.page || {}).after) || 0 : what === "refresh" ? HQ_CHAT.after : 0;
      HQ_CHAT.page = null;
      HQ_CHAT.id = Number(id); HQ_CHAT.after = after;
      HQ_CHAT.page = await api("GET", `/v1/hq/conversations/${Number(id)}?after=${after}`);
      if (what === "open") HQ_CHAT.assistance = null;
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
