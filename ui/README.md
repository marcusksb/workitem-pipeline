# Workload

Local web UI for the workitem pipeline. Design: `../workitem-pipeline-ui.md`.

## Run

```
npm install        # also installs frontend/
npm run dev        # needs cargo-watch: cargo install cargo-watch
```

One command: `vite build --watch` rebuilds the frontend, `cargo watch` rebuilds and restarts the backend on every change. Reload the browser after a frontend change. State lives in files and tmux, so a backend restart loses nothing.

Release binary instead: `npm run build`, then `backend/target/release/workload`. Stop any running instance first; both use port 7373.

Open the printed `http://127.0.0.1:7373/#token=…` URL once. `workload --rotate-token` replaces the token.

Blocked items raise a browser notification with a beep while the UI tab is open. Click anywhere on the page once after loading it, so the browser allows sound and asks for notification permission. The backend needs your login environment (`claude`, `tmux`, `python3` on `PATH`), so start it from your shell.

The dispatcher only runs while the backend runs. It starts `claude` sessions for `backlog` and `analyze` items as soon as the backend runs, so turn off `/loop /workitem-run` for those stages first. `WORKITEM_UI_DISPATCH=off` disables it.

Config: `~/.config/workitem-ui/config.toml` (override with `WORKITEM_UI_CONFIG`). Port: `WORKITEM_UI_PORT`. Other tmux server: `WORKITEM_UI_TMUX_SOCKET`.

## Test

```
npm test                                               # cargo test (real tmux) + vitest
cd frontend && npm run build && npm run e2e            # playwright; needs `cargo build` first
```
