//! The tokio runtime the core runs on, and the way its results come back to the main thread.
//! Every network call, timer, file read and decode runs on tokio; `nd::system::spawn` hands the
//! output to the UI thread with the app in hand.

use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use nd::prelude::*;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn handle() -> tokio::runtime::Handle {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("messages-rt")
                .enable_all()
                .build()
                .expect("build the tokio runtime")
        })
        .handle()
        .clone()
}

/// Runs `future` on tokio, then `done` with its output on the main thread.
pub fn spawn<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
    done: impl FnOnce(T, &mut App) + 'static,
) {
    let task = handle().spawn(future);
    nd::system::spawn(async move { task.await.ok() }, move |out, cx| {
        if let Some(out) = out {
            done(out, cx);
        }
    });
}

/// As [`spawn`], delivering to `entity` while it is alive.
pub fn spawn_for<V: 'static, T: Send + 'static>(
    entity: WeakEntity<V>,
    future: impl Future<Output = T> + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    spawn(future, move |out, cx| {
        entity.update(cx, |this, cx| done(this, out, cx));
    });
}

/// Fire and forget on tokio.
pub fn detach(future: impl Future<Output = ()> + Send + 'static) {
    handle().spawn(future);
}

/// Calls `done` on the main thread after `delay`. The sleep is made inside the task: a timer made
/// on the main thread panics when AppKit renders from a callback outside the runtime's context.
pub fn after(delay: Duration, done: impl FnOnce(&mut App) + 'static) {
    spawn(async move { tokio::time::sleep(delay).await }, move |(), cx| done(cx));
}

/// Pumps a channel filled on another thread into `handle` on the main thread, a batch per hop,
/// until the sender goes away.
pub fn pump<T: Send + 'static>(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<T>,
    handle: impl Fn(T, &mut App) + 'static,
) {
    let handle = std::rc::Rc::new(handle);
    fn next<T: Send + 'static>(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<T>,
        handle: std::rc::Rc<dyn Fn(T, &mut App)>,
    ) {
        spawn(
            async move {
                let first = rx.recv().await?;
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                Some((batch, rx))
            },
            move |out, cx| {
                let Some((batch, rx)) = out else { return };
                for item in batch {
                    handle(item, cx);
                }
                next(rx, handle);
            },
        );
    }
    let _ = &mut rx;
    next(rx, handle);
}

pub enum Claim {
    Primary(nd_ipc::instance::Primary),
    Forwarded,
}

/// The single-instance lock: the first launch keeps it, a later one forwards and exits. `None`
/// when the lock cannot be taken at all, in which case the app runs anyway.
pub fn claim_instance() -> Option<Claim> {
    let endpoint = nd_ipc::Endpoint::for_app("messages", "instance");
    let launch = nd_ipc::instance::Launch::from_env();
    match handle().block_on(nd_ipc::instance::acquire(&endpoint, &launch)) {
        Ok(nd_ipc::instance::Instance::Primary(primary)) => Some(Claim::Primary(primary)),
        Ok(nd_ipc::instance::Instance::Forwarded) => Some(Claim::Forwarded),
        Err(error) => {
            tracing::warn!("single instance: {error}");
            None
        }
    }
}

/// Brings the window forward whenever a second launch hands over.
pub fn raise_on_launch(mut primary: nd_ipc::instance::Primary, cx: &mut App) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    handle().spawn(async move {
        while primary.next_launch().await.is_some() {
            if tx.send(()).is_err() {
                break;
            }
        }
    });
    let _ = cx;
    pump(rx, |(), cx| {
        let window = cx.window_ids().next();
        if let Some(window) = window {
            cx.activate_window(window);
        }
    });
}
