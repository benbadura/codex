use super::*;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use tokio::sync::watch;

#[tokio::test(start_paused = true)]
async fn event_wait_has_no_deadline_and_wakes_on_mail() {
    let (tx, mut rx) = watch::channel(InputQueueActivity::Mailbox);
    let wait = wait_for_activity(
        &mut rx, /*pending_activity*/ None, /*deadline*/ None,
    );
    tokio::pin!(wait);
    assert!(wait.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(3_600)).await;
    assert!(wait.as_mut().now_or_never().is_none());
    tx.send_replace(InputQueueActivity::Mailbox);
    assert_eq!(wait.await.unwrap(), WaitOutcome::MailboxActivity);
}

#[tokio::test]
async fn queued_mail_and_steering_complete_without_waiting() {
    let (_tx, mut rx) = watch::channel(InputQueueActivity::Mailbox);
    for (activity, expected) in [
        (InputQueueActivity::Mailbox, WaitOutcome::MailboxActivity),
        (InputQueueActivity::Steer, WaitOutcome::Steered),
    ] {
        assert_eq!(
            wait_for_activity(&mut rx, Some(activity), /*deadline*/ None)
                .now_or_never()
                .unwrap()
                .unwrap(),
            expected,
        );
    }
}

#[tokio::test]
async fn event_wait_wakes_on_new_steering() {
    let (tx, mut rx) = watch::channel(InputQueueActivity::Mailbox);
    let wait = wait_for_activity(
        &mut rx, /*pending_activity*/ None, /*deadline*/ None,
    );
    tokio::pin!(wait);
    assert!(wait.as_mut().now_or_never().is_none());
    tx.send_replace(InputQueueActivity::Steer);
    assert_eq!(wait.await.unwrap(), WaitOutcome::Steered);
}

#[tokio::test]
async fn closed_activity_channel_is_an_error() {
    let (tx, mut rx) = watch::channel(InputQueueActivity::Mailbox);
    drop(tx);
    assert!(matches!(
        wait_for_activity(&mut rx, /*pending_activity*/ None, /*deadline*/ None).await,
        Err(FunctionCallError::RespondToModel(message)) if message == "Agent activity channel closed."
    ));
}
