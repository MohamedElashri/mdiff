# Needs attention

Open **Needs attention** from the toolbar/palette. Configure sources in **Preferences → Monitoring**, then enable monitoring. Monitoring, OS alerts, the tray and keeping closed windows in the tray are separate opt-ins. Credentials remain in **Preferences → Connections**; conflicting preference saves require reload.

The inbox prioritizes requested reviews, failed checks, index conflicts and hook-reported input requests. **Mark read** changes the badge; **Dismiss** hides without resolving; history can restore items. Only proven resolution followed by recurrence makes an item unread again. Outages, incomplete pages and unknown agent states cannot resolve items. Paused/removed sources retain history.

## Monitoring and destinations

One [native coordinator](../../src-tauri/src/attention/mod.rs) serves windows/tray without starting review/LSP runtimes. It coalesces roots and monitors at most 32 local tabs, 16 remote repositories and 64 known check targets.

- At most one local read and one remote job per 15 seconds; selected/background due intervals are normally 2/10 minutes with fair rotation.
- Remote jobs time out after 20 seconds. No log, conversation or history polling; authentication discovery is cached per host/connection epoch.
- Disabled/empty monitoring has no timer. Connection changes cancel reads/clear caches; quit cancels background work and rejects late observations.
- History retains 200 full items plus bounded compact read/dismiss markers. Corrupt files are preserved; runtime identities never restore from disk.

Actions resolve native item IDs and revalidate checkout/window ownership. Conflicts open recovery, requested reviews use the explicit opening flow, and checks target the **recorded head**. Agent actions reveal only an owned live terminal. Actions never grant trust or launch shells.

## Notifications and close

Alerts contain generic text and item IDs; delivery depends on OS services. Attempts have a one-minute cooldown and 16-per-run cap. Stale destinations fall back to the inbox.

Keep-in-tray hides the existing window with its reviews/PTYs alive; it requires a working tray. Quit reveals windows and uses normal save/terminal confirmation. There is no autostart or separate daemon. See [validation](../../VALIDATION.md).
