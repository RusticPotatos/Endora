// ---- boot -----------------------------------------------------------------
(async function () {
  try {
    const health = await api("GET", "/health");
    setupHeader(health);
    await reload();
    subscribeToActivity();
  } catch (e) {
    // Say which half failed. This catch covers reaching the node AND drawing the page, and
    // it reported both as "couldn't reach the node" — so a missing function read as a
    // network problem and sent the diagnosis in the wrong direction for a while.
    const unreachable = e instanceof TypeError && /fetch|network|load failed/i.test(e.message);
    app.innerHTML = unreachable
      ? `<div class="msg show err">Couldn't reach the node: ${esc(e.message)}</div>`
      : `<div class="msg show err">Endora is running, but the console failed to draw:
           ${esc(e.message)}</div>`;
  }
})();
