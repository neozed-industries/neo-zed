pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Neo Zed is running in stateless mode.
/// When true, Neo Zed will use in-memory databases instead of persistent storage.
pub static ZED_STATELESS: LazyLock<bool> = bool_env_var!("ZED_STATELESS");
