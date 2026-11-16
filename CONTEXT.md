# CodexBar applet for COSMIC

A COSMIC panel applet that shows AI provider usage computed by CodexBar for Linux. It only displays what the
desktop app gives it; see `docs/adr/0001-adapter-over-codexbar-linux.md`.

## Language

### Around the applet

**Desktop app**:
Upstream's CodexBar for Linux (`codexbar-linux`), running on the host outside the applet's sandbox.
_Avoid_: daemon, server, host app

**Backend**:
The desktop app in its role as the applet's only data source, reached over its local socket.
_Avoid_: service, API

**Adapter**:
A panel integration that displays the backend's data without computing any of its own, such as this applet or
upstream's Omarchy widget.
_Avoid_: plugin, client

**Tray icon**:
The desktop app's own status icon, which duplicates the applet in COSMIC's status area.
_Avoid_: indicator, applet

### What the backend sends

**Snapshot**:
One complete, versioned picture of usage, preferences and backend state, sent in reply to a request.
_Avoid_: status, payload, state

**Provider**:
An AI service whose usage CodexBar tracks, identified by a lowercase id such as `claude` or `codex`.
_Avoid_: account, service, model

**Entry**:
One provider's usage within a snapshot: its plan, status, windows and credits.
_Avoid_: provider card, row

**Window**:
One quota period of an entry, such as "5 hour" or "7 day", with its percentage and reset time.
_Avoid_: limit, quota, bucket

**Scoped cap**:
A window that applies to only part of a provider, such as a single model.
_Avoid_: model limit, extra window

**Bar entry**:
One provider's short, preformatted text for the panel, with a tag such as `CL`.
_Avoid_: label, chip

**Summary**:
The single preformatted line that older backends give instead of bar entries.
_Avoid_: title, status text

**Spending**:
A provider's local cost for today and the last 30 days, scanned from local logs by the desktop app.
_Avoid_: cost, billing

**Stale**:
Said of a snapshot whose last refresh failed, or whose data is older than twice the refresh interval (at least
10 minutes).
_Avoid_: outdated, expired

### What the applet shows

**Connecting**:
The first seconds after the applet starts, before any snapshot has arrived.

**Unavailable**:
The backend can't be reached, whether the desktop app isn't installed, isn't running or isn't responding.
_Avoid_: offline, disconnected, not installed

**Incompatible**:
The backend replied, but in a schema version or shape this applet doesn't understand.
_Avoid_: unsupported, broken

**Connected**:
The applet has a usable snapshot.
