pub mod actions;
pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod dispatch;
pub mod files;
pub mod git;
pub mod launch;
pub mod locate;
pub mod pipeline;
pub mod plans;
pub mod review;
pub mod static_files;
pub mod terminal;
pub mod tmux;
pub mod watch;
pub mod workitem;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};

pub struct AppState {
    pub store: config::Store,
    /// `tmux -L <socket>`; `None` uses the default server.
    pub tmux_socket: Option<String>,
    pub port: u16,
    /// One more host the guard accepts besides `127.0.0.1` and `localhost` (`WORKITEM_UI_HOST`,
    /// decision 56): a LAN address or a tailnet name, so a phone can connect.
    pub extra_host: Option<String>,
    /// Holds `workitem-pipeline/{pipeline.json,hooks.json,bin/workitem-move}`.
    pub skills_dir: PathBuf,
    /// Executable that runs a handler session; `claude` unless a test swaps in a stub.
    pub claude: String,
    /// Executable that reads PR state; `gh` unless a test swaps in a stub.
    pub gh: String,
    /// Last valid board per `workspace/board`; see `AppState::pipeline`.
    pub pipelines: std::sync::Mutex<HashMap<String, pipeline::Pipeline>>,
    /// Serialises session launches and dispatcher passes so an item is never started twice.
    pub busy: tokio::sync::Mutex<()>,
    /// Keys (`workspace/board/stage`) of column watchers whose script is running, so one never overlaps itself.
    pub watching: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Wakes the dispatcher ahead of its tick.
    pub wake: tokio::sync::Notify,
    /// Stages (`workspace/board/stage`) whose watchers do not start; see `watch::set_paused`.
    pub paused: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl AppState {
    /// Defaults plus the `WORKITEM_UI_*` environment overrides.
    pub fn new(store: config::Store, port: u16) -> Self {
        let env = |name: &str| std::env::var(name).ok();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        AppState {
            tmux_socket: env("WORKITEM_UI_TMUX_SOCKET"),
            port,
            extra_host: env("WORKITEM_UI_HOST").filter(|h| !h.is_empty()),
            skills_dir: env("WORKITEM_UI_SKILLS").map(PathBuf::from).unwrap_or_else(|| home.join(".claude/skills")),
            claude: env("WORKITEM_UI_CLAUDE").unwrap_or_else(|| "claude".into()),
            gh: env("WORKITEM_UI_GH").unwrap_or_else(|| "gh".into()),
            pipelines: Default::default(),
            busy: Default::default(),
            watching: Default::default(),
            wake: Default::default(),
            paused: std::sync::Mutex::new(watch::load_paused(&store)),
            store,
        }
    }
}

pub type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/login", post(api::login))
        .route("/api/me", get(api::me))
        .route("/api/workspaces", get(api::list_workspaces).post(api::add_workspace))
        .route("/api/workspaces/{name}", put(api::update_workspace))
        .route("/api/workspaces/{workspace}/boards", post(api::add_board))
        .route("/api/pipelines", get(api::pipelines))
        .route("/api/workspaces/{workspace}/boards/{board}/stages/{stage}/source", get(api::stage_source))
        .route("/api/workspaces/{workspace}/boards/{board}/stages/{stage}/paused", put(api::pause_stage))
        .route("/api/workspaces/{workspace}/boards/{board}/learnings", get(api::learnings).put(api::save_learnings))
        .route("/api/workitems", get(api::list_items))
        .route("/api/workitems/{workspace}/{dir}", get(api::item))
        .route("/api/workitems/{workspace}/{dir}/pr", get(api::pr_conversation))
        .route("/api/workitems/{workspace}/{dir}/chat", get(chat::item_chat).post(chat::item_send))
        .route("/api/workitems/{workspace}/{dir}/keys", post(chat::item_key))
        .route("/api/workitems/{workspace}/{dir}/files", get(files::list_files))
        .route("/api/workitems/{workspace}/{dir}/files/{*path}", get(files::file))
        .route("/api/workitems/{workspace}/{dir}/reveal", post(files::reveal))
        .route("/api/workitems/{workspace}/{dir}/approve", post(actions::approve))
        .route("/api/workitems/{workspace}/{dir}/reject", post(actions::reject))
        .route("/api/workitems/{workspace}/{dir}/brief", post(actions::edit_brief))
        .route("/api/workitems/{workspace}/{dir}/frontmatter", post(actions::edit_frontmatter))
        .route("/api/workitems/{workspace}/{dir}/clear-blocked", post(actions::clear_blocked))
        .route("/api/workitems/{workspace}/{dir}/kill-session", post(actions::kill_session))
        .route("/api/workitems/{workspace}/{dir}/start", post(actions::start))
        .route("/api/workitems/{workspace}/{dir}/run", post(actions::run_handler))
        .route("/api/workitems/{workspace}/{dir}/fix", post(actions::fix))
        .route("/api/workitems/{workspace}/{dir}/session", post(actions::start_session))
        .route("/api/workitems/{workspace}/{dir}/discuss", post(actions::discuss_brief))
        .route("/api/workitems/{workspace}/{dir}/merge",post(actions::merge))
        .route("/api/workitems/{workspace}/{dir}/rebase", post(actions::rebase))
        .route("/api/workitems/{workspace}/{dir}/remove", post(actions::remove))
        .route("/api/workspaces/{workspace}/create", post(actions::create))
        .route("/api/sessions", get(api::list_sessions))
        .route("/api/plans", get(plans::list_plans))
        .route("/api/plans/{workspace}/{id}", get(plans::plan).delete(plans::delete))
        .route("/api/plans/{workspace}/{id}/resume", post(plans::resume))
        .route("/api/plans/{workspace}/{id}/chat", get(chat::plan_chat).post(chat::plan_send))
        .route("/api/plans/{workspace}/{id}/keys", post(chat::plan_key))
        .route("/api/workspaces/{workspace}/plans", post(plans::create))
        .route("/ws/terminal/{workspace}/{dir}", get(terminal::attach_item))
        .route("/ws/session/{workspace}/{name}", get(terminal::attach_agent))
        .fallback(static_files::serve)
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        .with_state(state)
}
