use color_eyre::Result;

use crate::action::Action;
use crate::app::App;
use crate::app::helpers::GitOpGuard;
use crate::repo_id::RepoId;

impl App {
    pub(super) fn spawn_git_operation<F, M>(
        &self,
        repo_id: RepoId,
        operation: F,
        success_message: M,
        error_context: Option<&'static str>,
    ) where
        F: FnOnce() -> Result<String> + Send + 'static,
        M: FnOnce(String) -> String + Send + 'static,
    {
        let tx = self.action_tx.clone();
        tokio::task::spawn_blocking(move || {
            let guard = GitOpGuard::new(repo_id.clone(), tx.clone());
            match operation() {
                Ok(output) => {
                    guard.complete();
                    let _ = tx.send(Action::GitOpComplete {
                        id: repo_id,
                        message: success_message(output),
                    });
                }
                Err(e) => {
                    guard.complete();
                    let message = match error_context {
                        Some(context) => format!("{context}: {e}"),
                        None => format!("{e}"),
                    };
                    let _ = tx.send(Action::Notice(message));
                    let _ = tx.send(Action::RefreshRepo(repo_id));
                }
            }
        });
    }
}
