use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

static NEXT_TRANSFER_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_TRANSFERS: OnceLock<Mutex<HashMap<PathBuf, ActiveTransfer>>> = OnceLock::new();

struct ActiveTransfer {
    id: u64,
    token: CancellationToken,
}

pub(crate) struct AttachmentTransferRegistration {
    destination: PathBuf,
    id: u64,
    token: CancellationToken,
}

impl AttachmentTransferRegistration {
    pub(crate) fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for AttachmentTransferRegistration {
    fn drop(&mut self) {
        let mut transfers = registry().lock().unwrap_or_else(|error| error.into_inner());
        if transfers
            .get(&self.destination)
            .is_some_and(|active| active.id == self.id)
        {
            transfers.remove(&self.destination);
        }
    }
}

pub(crate) fn register(destination: &Path) -> AttachmentTransferRegistration {
    let destination = destination.to_path_buf();
    let id = NEXT_TRANSFER_ID.fetch_add(1, Ordering::Relaxed);
    let token = CancellationToken::new();
    let mut transfers = registry().lock().unwrap_or_else(|error| error.into_inner());
    if let Some(previous) = transfers.insert(
        destination.clone(),
        ActiveTransfer {
            id,
            token: token.clone(),
        },
    ) {
        previous.token.cancel();
    }
    AttachmentTransferRegistration {
        destination,
        id,
        token,
    }
}

pub(crate) fn cancel(destination: &Path) -> bool {
    let transfers = registry().lock().unwrap_or_else(|error| error.into_inner());
    let Some(active) = transfers.get(destination) else {
        return false;
    };
    active.token.cancel();
    true
}

fn registry() -> &'static Mutex<HashMap<PathBuf, ActiveTransfer>> {
    ACTIVE_TRANSFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_registration_cancels_only_the_previous_transfer() {
        let destination = std::env::temp_dir().join(format!(
            "im-core-cancellation-{}-{}",
            std::process::id(),
            NEXT_TRANSFER_ID.load(Ordering::Relaxed)
        ));
        let first = register(&destination);
        assert!(!first.token().is_cancelled());

        let second = register(&destination);
        assert!(first.token().is_cancelled());
        assert!(!second.token().is_cancelled());
        drop(first);

        assert!(cancel(&destination));
        assert!(second.token().is_cancelled());
        drop(second);
        assert!(!cancel(&destination));
    }
}
