use futures::future::select;
use futures::future::Either;
use futures::pin_mut;
use std::time::Duration;
use task_scope::cancelable;
use task_scope::Canceled;
use task_scope::Scope;
use tokio::time::delay_for;
#[tokio::test]
async fn test_scope() {
    let scope = Scope::new();
    let cancel_scope = scope.cancel_scope();
    let work = scope.run(|spawner| {
        async move {
            spawner.spawn(cancelable_delay(Duration::from_millis(3000)));
            delay_for(Duration::from_millis(1000)).await;
            println!("parent work done");
        }
    });
    pin_mut!(work);
    match select(work, delay_for(Duration::from_millis(2000))).await {
        Either::Left((w, _)) => println!("scope ret value: {:?}", w),
        Either::Right((_, task)) => {
            cancel_scope.cancel();
            let result = task.await;
            println!("after timeout, task result is {:?}", result);
        }
    }
}

#[tokio::test]
async fn test_scope_in_scope() {
    let scope = Scope::new();
    let cancel_scope = scope.cancel_scope();
    scope
        .run(|spawner| {
            async move {
                let inner_scope = Scope::new();
                spawner.spawn(async move {
                    cancelable_delay(Duration::from_millis(1000)).await;
                    println!("outer scope subtask done");
                    cancel_scope.force_cancel();
                    println!("cancel output scope");
                });

                let scope_result = inner_scope
                    .run(|s| {
                        async move {
                            let joiner = s.spawn(async {
                                cancelable_delay(Duration::from_millis(2000)).await;
                                println!("inner scope subtask done");
                            });
                            let _ = joiner.await;
                        }
                    })
                    .await;
                println!("inner scope result {:?}", &scope_result);
                assert!(scope_result.is_err());
                assert!(scope_result.unwrap_err() == Canceled::Forced);
            }
        })
        .await
        .expect("should run to end");
}
async fn cancelable_delay(dur: Duration) {
    match cancelable(delay_for(dur)).await {
        Ok(_) => println!("normal delayed"),
        Err(Canceled::Forced) => println!("delay force cancelled"),
        Err(Canceled::Graceful) => println!("delay grace cancelled"),
    }
}
