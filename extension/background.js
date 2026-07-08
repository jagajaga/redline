// Redline Usage Bridge — runs inside your logged-in browser, so it reaches the
// claude.ai usage endpoint the same way the Settings page does (past Cloudflare,
// with your existing session). It then POSTs the result to the local Redline
// daemon. Nothing is stored; nothing leaves your machine.

const DAEMON = "http://127.0.0.1:47615/usage";
const PERIOD_MIN = 2;

// Firefox exposes `browser`; Chrome exposes `chrome`. Both alias enough of the
// APIs we use, so normalize to whichever exists.
const api = typeof browser !== "undefined" ? browser : chrome;

async function getJSON(url) {
  const r = await fetch(url, { credentials: "include", headers: { accept: "*/*" } });
  if (!r.ok) throw new Error(`${url} -> ${r.status}`);
  return r.json();
}

async function poll() {
  let status = { ok: false, when: Date.now(), note: "" };
  try {
    // 1. Which organization? Prefer the last-active one.
    const orgs = await getJSON("https://claude.ai/api/organizations");
    const list = Array.isArray(orgs) ? orgs : orgs.organizations || [orgs];
    const org = list.find((o) => o && (o.uuid || o.id)) || list[0];
    const uuid = org && (org.uuid || org.id);
    if (!uuid) throw new Error("no organization uuid");

    // 2. The plan-usage limits (session + weekly).
    const usage = await getJSON(
      `https://claude.ai/api/organizations/${uuid}/usage`
    );

    // 3. Hand it to the daemon (raw — the daemon parses + persists it).
    await fetch(DAEMON, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ org: uuid, usage, at: Date.now() }),
    });
    status.ok = true;
    status.note = "forwarded";
  } catch (e) {
    status.note = String(e && e.message ? e.message : e);
  }
  api.storage.local.set({ lastStatus: status });
}

// Let the popup force an immediate poll and get the result back.
api.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg === "pollNow") {
    poll().then(() => api.storage.local.get("lastStatus").then((r) => sendResponse(r.lastStatus)));
    return true; // async response
  }
});

api.runtime.onInstalled.addListener(() => {
  api.alarms.create("poll", { periodInMinutes: PERIOD_MIN });
  poll();
});
if (api.runtime.onStartup) api.runtime.onStartup.addListener(poll);
api.alarms.onAlarm.addListener((a) => {
  if (a.name === "poll") poll();
});
// Poke once when the worker spins up.
poll();
