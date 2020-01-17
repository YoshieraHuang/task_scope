use crate::with_token::WithToken;
use crate::{cancellation, waker, Canceled, Join, Token};
use futures::future::poll_fn;
use futures::pin_mut;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

#[cfg(feature = "async-std")]
pub mod async_std;
#[cfg(feature = "async-std")]
pub use self::async_std::*;
#[cfg(feature = "tokio")]
pub mod tokio;
#[cfg(feature = "tokio")]
pub use self::tokio::*;
use crate::handle::JoinHandle;
use futures_intrusive::channel::shared::StateReceiver;

#[derive(Clone)]
pub struct ScopeSpawner {
    pub(crate) cancel: Arc<StateReceiver<bool>>,
    pub(crate) join: Arc<Join>,
}

impl ScopeSpawner {
    #[cfg(feature = "tokio")]
    pub fn spawn<Fut>(&self, task: Fut) -> JoinHandle<TokioJoinHandle<Fut::Output>>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        JoinHandle {
            inner: ::tokio::spawn(install_within_scope(task, self.clone())),
        }
    }

    #[cfg(feature = "async-std")]
    pub fn spawn<Fut>(&self, task: Fut) -> JoinHandle<AsyncStdJoinHandle<Fut::Output>>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        let inner = async_std::task::spawn(install_within_scope(task, self.clone()));
        JoinHandle {
            inner: AsyncStdJoinHandle(inner),
        }
    }
}

/// Installs the current context in the given future.
///
/// The returned future receives a cancellation signal from the current context and terminates when
/// it encounters a forced cancellation.
pub fn install<'f, T, F>(
    future: F,
    cx: &mut Context,
) -> impl Future<Output = Result<T, Canceled>> + 'f
where
    T: 'f,
    F: Future<Output = T> + 'f,
{
    let data = unsafe { waker::retrieve_data(cx).expect("must be polled in a scope") };
    install_within_scope(
        future,
        ScopeSpawner {
            join: Weak::upgrade(&data.token.join).expect("no task is running"),
            cancel: data.token.cancel.clone(),
        },
    )
}

pub(crate) fn install_within_scope<'f, T, F>(
    future: F,
    scope_spawner: ScopeSpawner,
) -> impl Future<Output = Result<T, Canceled>> + 'f
where
    F: Future<Output = T> + 'f,
    T: 'f,
{
    let ScopeSpawner { join, cancel } = scope_spawner;
    async move {
        // introduce cancellation points at every yield
        let future = WithToken::new(async move {
            pin_mut!(future);

            let cancellation = cancellation();
            pin_mut!(cancellation);

            poll_fn(|cx| {
                // stop the task only if a forced cancellation is issued
                // the tasks can continue running on a graceful cancellation
                // so that they can perform custom cancellation logic
                if let Poll::Ready(Some(Canceled::Forced)) = cancellation.as_mut().poll(cx) {
                    return Poll::Ready(Err(Canceled::Forced));
                }

                future.as_mut().poll(cx).map(Ok)
            })
            .await
        });
        pin_mut!(future);

        poll_fn(|cx| {
            let token = Token {
                cancel: cancel.clone(),
                join: Arc::downgrade(&join),
            };
            future.as_mut().poll(cx, token)
        })
        .await
    }
}
