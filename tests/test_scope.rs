use std::time::Duration;
use task_scope::Scope;
use tokio::time::{delay_for, timeout};

#[tokio::test]
async fn test_scope() {
    let scope = Scope::new();
    let cancel_scope = scope.cancel_scope();
    scope
        .run(|spawner, cancel_scope| {
            async move {
                let join_handle = spawner.spawn(async {
                    // TODO: know if force cancel
                    delay_for(Duration::from_millis(5000)).await;
                    println!("child is done");
                });
                delay_for(Duration::from_millis(2000)).await;
                cancel_scope.cancel();
                println!("parent is done");
            }
        })
        .await
        .expect("must complete");
    println!("scope is gone");
    delay_for(Duration::from_millis(10000)).await;
}
