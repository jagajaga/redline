const api = typeof browser !== "undefined" ? browser : chrome;
const el = document.getElementById("status");
api.storage.local.get("lastStatus").then((r) => {
  const s = r.lastStatus;
  if (!s) {
    el.textContent = "No poll yet — open claude.ai while logged in.";
    return;
  }
  const ago = Math.round((Date.now() - s.when) / 1000);
  if (s.ok) {
    el.innerHTML = `<span class="ok">✓ forwarded to Redline</span><br><span class="muted">${ago}s ago</span>`;
  } else {
    el.innerHTML = `<span class="bad">✗ ${s.note}</span><br><span class="muted">${ago}s ago — is the Redline daemon running & are you logged into claude.ai?</span>`;
  }
});
