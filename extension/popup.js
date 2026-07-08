const api = typeof browser !== "undefined" ? browser : chrome;
const el = document.getElementById("status");

function render(s) {
  if (!s) {
    el.textContent = "No poll yet — open claude.ai while logged in.";
    return;
  }
  const ago = Math.max(0, Math.round((Date.now() - s.when) / 1000));
  if (s.ok) {
    el.innerHTML = `<span class="ok">✓ forwarded to Redline</span><br><span class="muted">${ago}s ago</span>`;
  } else {
    el.innerHTML = `<span class="bad">✗ ${s.note}</span><br><span class="muted">${ago}s ago — is Redline running & are you logged into claude.ai?</span>`;
  }
}

// Show the last status immediately, then force a fresh poll on open.
api.storage.local.get("lastStatus").then((r) => render(r.lastStatus));
el.textContent = "checking…";
api.runtime.sendMessage("pollNow").then(render).catch(() => {
  api.storage.local.get("lastStatus").then((r) => render(r.lastStatus));
});
