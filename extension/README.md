# Redline Usage Bridge

A tiny browser extension that reads your **Claude plan-usage limits** (the exact
`session %` / `weekly %` from `claude.ai/settings/usage`) and forwards them to
the local Redline daemon, so the Governor matches Claude exactly.

**Why an extension?** That usage endpoint is behind Cloudflare's browser check —
`curl` or a background daemon get a 403. Running *inside* your logged-in browser
is the only clean way past it, and it means **no credentials are ever stored**:
the extension just piggybacks on the session you're already signed into. Nothing
leaves your machine — it only talks to `claude.ai` and `127.0.0.1`.

## Install

The daemon must be running (open Redline). Then load the extension unpacked:

**Chrome / Edge / Brave**
1. `chrome://extensions` → enable **Developer mode** (top-right).
2. **Load unpacked** → select this `extension/` folder.
3. Open/refresh a `claude.ai` tab while logged in.

**Firefox**
1. `about:debugging#/runtime/this-firefox`.
2. **Load Temporary Add-on…** → select `extension/manifest.json`.
   (Temporary add-ons unload when Firefox restarts; reload it after a restart,
   or install a signed build once we publish one.)
3. Open/refresh a `claude.ai` tab while logged in.

Click the extension's toolbar icon to see status (`✓ forwarded` or the error).

## How it works

Every ~2 minutes (and on browser start) the background worker:
1. `GET https://claude.ai/api/organizations` → your org id,
2. `GET https://claude.ai/api/organizations/<id>/usage` → the limits,
3. `POST` that JSON to `http://127.0.0.1:47615/usage` (the Redline daemon),

which parses the session + weekly percentages and feeds them to the Governor
with top priority (live API > transcript banner > config > learned).
