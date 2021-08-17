use std::time::Duration;
use tokio::time::{sleep, timeout};

use task_scope::{scope, spawn};

#[tokio::test]
async fn test_basic() {
    scope(async {
        spawn(async {
            sleep(Duration::from_millis(500)).await;

            println!("child is done");
        })
        .await;
        println!("parent is done");
    })
    .await
    .expect("must complete");

    sleep(Duration::from_millis(1000)).await;
}

#[tokio::test]
async fn test_drop() {
    timeout(
        Duration::from_millis(50),
        scope(async {
            spawn(async {
                println!("child started");
                sleep(Duration::from_millis(100)).await;

                panic!("child is canceled");
            })
            .await;
        }),
    )
    .await
    .unwrap_err();

    sleep(Duration::from_millis(60)).await;
}
