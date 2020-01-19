use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::FutureExt;
use futures::StreamExt;
use net2::TcpBuilder;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use task_scope::{CancelScope, Scope, ScopeSpawner};
use tokio::net::{self, TcpStream};
use tokio::time::delay_for;

const HAPPY_EYEBALL_DEFAULT_DELAY: u64 = 250;

#[tokio::main]
async fn main() {
    let host = "baidu.com:80";
    let ipaddrs: Vec<SocketAddr> = net::lookup_host(host)
        .await
        .expect("lookup host should be ok")
        .collect::<Vec<_>>();
    println!("{:?}", &ipaddrs);
    let scope = Scope::new();
    let cancel_token = scope.cancel_scope();

    let conn = scope
        .run(|spawner| happy_eyeball_connect(ipaddrs, spawner, cancel_token))
        .await;

    if let Ok(Some(c)) = conn {
        println!("connected: {:?}, {:?}", c.local_addr(), c.peer_addr());
    } else {
        println!("no connection");
    }
}

async fn happy_eyeball_connect(
    ipaddrs: Vec<SocketAddr>,
    spawner: ScopeSpawner,
    cancel_token: CancelScope,
) -> Option<TcpStream> {
    let (conn_tx, conn_rx) = mpsc::channel(1);
    let mut conn_rx = conn_rx.fuse();
    for addr in ipaddrs {
        let mut conn_tx_clone = conn_tx.clone();
        let addr_clone = addr.clone();
        let (failure_tx, failure_rx) = oneshot::channel();
        println!("spawn task for addr {:?}", &addr_clone);
        spawner.spawn(async move {
            let conn_result = connect(
                addr_clone.clone(),
                None,
                true,
                Some(Duration::from_millis(1000)),
            )
            .await;
            match conn_result {
                Ok(c) => {
                    if let Err(_e) = conn_tx_clone.try_send(c) {
                        println!("other task already connected, this addr: {}", &addr_clone);
                    } else {
                        conn_tx_clone.close_channel();
                        drop(conn_tx_clone);
                        drop(failure_tx);
                    }
                }
                Err(e) => {
                    let _ = failure_tx.send((addr_clone, e));
                }
            }
        });

        let mut timeout = delay_for(Duration::from_millis(HAPPY_EYEBALL_DEFAULT_DELAY)).fuse();
        let mut failure_rx = failure_rx.fuse();

        loop {
            futures::select! {
                conn = conn_rx.next() => {
                    if let Some(conn) = conn {
                        println!("success connect to {:?}", &conn.peer_addr());
                        cancel_token.force_cancel();
                        return Some(conn);
                    } else {
                        return None;
                    }
                }
                e = failure_rx => {
                    if let Ok((addr, e)) = e {
                        println!("connect to addr {:?} failed: {}", &addr, e);
                        // break to start next connection attempt
                        break;
                    } else {
                        // failure tx is dropped, loop again to get conn from conn_rx
                    }
                }
                _ = timeout => {
                    println!("timeout waiting connect to {}", &addr);
                    // break to start next connection attempt
                    break;
                }
            }
        }
    }

    // at last, wait for another 250ms
    let mut timeout = delay_for(Duration::from_millis(HAPPY_EYEBALL_DEFAULT_DELAY)).fuse();

    loop {
        futures::select! {
            conn = conn_rx.next() => {
                if let Some(c) = conn {
                    println!("success connect to {:?}", &c.peer_addr());
                    cancel_token.force_cancel();
                    return Some(c);
                } else {
                    // no sub tasks running, return directly
                    return None;
                }
            }
            _ = timeout => {
                println!("timeout waiting all connection attempt, exit now");
                return None;
            }
        }
    }
}

async fn connect(
    addr: SocketAddr,
    local_addr: Option<IpAddr>,
    reuse_address: bool,
    connect_timeout: Option<Duration>,
) -> io::Result<TcpStream> {
    let builder = match addr {
        SocketAddr::V4(_) => TcpBuilder::new_v4()?,
        SocketAddr::V6(_) => TcpBuilder::new_v6()?,
    };

    if reuse_address {
        builder.reuse_address(reuse_address)?;
    }

    if let Some(local_addr) = local_addr {
        // Caller has requested this socket be bound before calling connect
        builder.bind(SocketAddr::new(local_addr, 0))?;
    } else if cfg!(windows) {
        // Windows requires a socket be bound before calling connect
        let any: SocketAddr = match addr {
            SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
            SocketAddr::V6(_) => ([0, 0, 0, 0, 0, 0, 0, 0], 0).into(),
        };
        builder.bind(any)?;
    }

    let std_tcp = builder.to_tcp_stream()?;

    let connect = TcpStream::connect_std(std_tcp, &addr);
    match connect_timeout {
        Some(dur) => match tokio::time::timeout(dur, connect).await {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(io::Error::new(io::ErrorKind::TimedOut, e)),
        },
        None => connect.await,
    }
}
