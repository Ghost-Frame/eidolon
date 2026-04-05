pub mod daemon;
pub mod local;

use crate::app::App;
use crate::daemon::client::DaemonClient;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Handle a slash command. Returns true if the command was recognized.
pub fn handle_command(
    app: &mut App,
    msg: &str,
    daemon_client: &Option<Arc<DaemonClient>>,
    system_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    if daemon::handle(app, msg, daemon_client, system_tx) {
        return true;
    }
    local::handle(app, msg)
}
