use std::future::Future;

use crate::error::{SafeError, SafeResult};

const MAX_REASON_CHARACTERS: usize = 256;

pub(crate) async fn confirm(reason: String) -> SafeResult<bool> {
    confirm_with(reason, confirm_platform).await
}

async fn confirm_with<F, Fut>(reason: String, evaluator: F) -> SafeResult<bool>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = SafeResult<bool>>,
{
    let reason = reason.trim().to_owned();
    if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARACTERS {
        return Err(SafeError::new(
            "invalid_input",
            "The user-presence reason is invalid.",
            false,
        ));
    }
    evaluator(reason).await
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
async fn confirm_platform(reason: String) -> SafeResult<bool> {
    tokio::task::spawn_blocking(move || {
        use std::sync::mpsc;
        use std::time::Duration;

        use block2::RcBlock;
        use objc2::runtime::Bool;
        use objc2_foundation::{NSError, NSString};
        use objc2_local_authentication::{LAContext, LAPolicy};

        let context = unsafe { LAContext::new() };
        let policy = LAPolicy::DeviceOwnerAuthentication;
        if unsafe { context.canEvaluatePolicy_error(policy) }.is_err() {
            return false;
        }

        let localized_reason = NSString::from_str(&reason);
        let (sender, receiver) = mpsc::channel();
        let reply = RcBlock::new(move |success: Bool, _error: *mut NSError| {
            let _ = sender.send(success.as_bool());
        });

        // SAFETY: objc2 supplies the exact LocalAuthentication signature. The
        // block captures only a thread-safe channel sender, and context,
        // reason and block remain alive until the callback or timeout.
        unsafe {
            context.evaluatePolicy_localizedReason_reply(policy, &localized_reason, &reply);
        }
        receiver
            .recv_timeout(Duration::from_secs(300))
            .unwrap_or(false)
    })
    .await
    .map_err(|_| SafeError::internal())
}

#[cfg(not(target_os = "macos"))]
async fn confirm_platform(_reason: String) -> SafeResult<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::SafeResult;

    #[tokio::test]
    async fn invalid_reasons_fail_before_platform_authentication() {
        for reason in [String::new(), " ".to_owned(), "x".repeat(257)] {
            let calls = AtomicUsize::new(0);
            let error = super::confirm_with(reason, |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                async { Ok(true) }
            })
            .await
            .unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(!error.retryable);
            assert_eq!(calls.load(Ordering::Relaxed), 0);
        }
    }

    #[tokio::test]
    async fn platform_result_is_returned_after_one_evaluation() {
        for expected in [false, true] {
            let calls = AtomicUsize::new(0);
            let result = super::confirm_with("  approve transfer  ".to_owned(), |reason| {
                calls.fetch_add(1, Ordering::Relaxed);
                assert_eq!(reason, "approve transfer");
                async move { SafeResult::Ok(expected) }
            })
            .await
            .unwrap();
            assert_eq!(result, expected);
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        }
    }
}
