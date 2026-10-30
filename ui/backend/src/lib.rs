pub mod actions;
pub mod api;
pub mod auth;
pub mod config;
pub mod dispatch;
pub mod files;
pub mod git;
pub mod launch;
pub mod locate;
pub mod pipeline;
pub mod review;
pub mod static_files;
pub mod terminal;
pub mod tmux;
pub mod watch;
pub mod workitem;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};

pub struct AppState {
    pub store: config::Store,
    /// `tmux -L <socket>`; `None` uses the default server.
    pub tmux_socket: Option<String>,
    pub port: u16,
    /// Holds `workitem-pipeline/{pipeline.json,hooks.json,bin/workitem-move}`.
    pub skills_dir: PathBuf,
    /// Executable that runs a handler session; `claude` unless a test swaps in a stub.
    pub claude: String,
    /// Executable that reads PR state; `gh` unless a test swaps in a stub.
    pub gh: String,
    /// Last valid board per project name; see `AppState::pipeline`.
    pub pipelines: std::sync::Mutex<HashMap<String, pipeline::Pipeline>>,
    /// Latest PR status per `project/dir`, filled by the dispatcher for `in_review` items.
    pub reviews: std::sync::Mutex<HashMap<String, review::Review>>,
    /// Serialises session launches and dispatcher passes so an item is never started twice.
    pub busy: tokio::sync::Mutex<()>,
    /// Keys (`project/board/stage`) of column watchers whose script is running, so one never overlaps itself.
    pub watching: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Wakes the dispatcher ahead of its tick.
    pub wake: tokio::sync::Notify,
    /// Stages (`project/board/stage`) whose watchers do not start; see `watch::set_paused`.
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
            skills_dir: env("WORKITEM_UI_SKILLS").map(PathBuf::from).unwrap_or_else(|| home.join(".claude/skills")),
            claude: env("WORKITEM_UI_CLAUDE").unwrap_or_else(|| "claude".into()),
            gh: env("WORKITEM_UI_GH").unwrap_or_else(|| "gh".into()),
            pipelines: Default::default(),
            reviews: Default::default(),
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
        .route("/api/projects", get(api::list_projects).post(api::add_project))
        .route("/api/projects/{name}", delete(api::remove_project))
        .route("/api/pipelines", get(api::pipelines))
        .route("/api/projects/{project}/boards/{board}/stages/{stage}/source", get(api::stage_source))
        .route("/api/projects/{project}/boards/{board}/stages/{stage}/paused", put(api::pause_stage))
        .route("/api/workitems", get(api::list_items))
        .route("/api/workitems/{project}/{dir}", get(api::item))
        .route("/api/workitems/{project}/{dir}/pr", get(api::pr_conversation))
        .route("/api/workitems/{project}/{dir}/files", get(files::list_files))
        .route("/api/workitems/{project}/{dir}/files/{*path}", get(files::file))
        .route("/api/workitems/{project}/{dir}/reveal", post(files::reveal))
        .route("/api/workitems/{project}/{dir}/approve", post(actions::approve))
        .route("/api/workitems/{project}/{dir}/reject", post(actions::reject))
        .route("/api/workitems/{project}/{dir}/brief", post(actions::edit_brief))
        .route("/api/workitems/{project}/{dir}/frontmatter", post(actions::edit_frontmatter))
        .route("/api/workitems/{project}/{dir}/clear-blocked", post(actions::clear_blocked))
        .route("/api/workitems/{project}/{dir}/kill-session", post(actions::kill_session))
        .route("/api/workitems/{project}/{dir}/start", post(actions::start))
        .route("/api/workitems/{project}/{dir}/run", post(actions::run_handler))
        .route("/api/workitems/{project}/{dir}/fix", post(actions::fix))
        .route("/api/workitems/{project}/{dir}/review", put(actions::set_review).delete(actions::clear_review))
        .route("/api/workitems/{project}/{dir}/session", post(actions::start_session))
        .route("/api/workitems/{project}/{dir}/discuss", post(actions::discuss_brief))
        .route("/api/workitems/{project}/{dir}/merge",post(actions::merge))
        .route("/api/workitems/{project}/{dir}/rebase", post(actions::rebase))
        .route("/api/workitems/{project}/{dir}/remove", post(actions::remove))
        .route("/api/projects/{project}/create", post(actions::create))
        .route("/api/sessions", get(api::list_sessions))
        .route("/ws/terminal/{project}/{dir}", get(terminal::attach_item))
        .route("/ws/session/{project}/{name}", get(terminal::attach_agent))
        .fallback(static_files::serve)
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        .with_state(state)
}
