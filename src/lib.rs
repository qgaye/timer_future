use std::{sync::{Arc, Mutex, mpsc::{SyncSender, sync_channel, Receiver}}, task::{Waker, Context, Poll}, pin::Pin, time::Duration, thread};

use futures::{Future, future::{BoxFuture}, FutureExt, task::{ArcWake, waker_ref}};


struct TimeFuture {
    shared_state: Arc<Mutex<SharedState>>,
}

struct SharedState {
    completed: bool,
    waker: Option<Waker>,
}

impl Future for TimeFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared_state = self.shared_state.lock().unwrap();
        if shared_state.completed {
            Poll::Ready(())
        } else {
            shared_state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl TimeFuture {
    pub fn new(duration: Duration) -> Self {
        let shared_state = Arc::new(Mutex::new(
            SharedState {
                completed: false,
                waker: None,
            }
        ));

        let thread_shared_state = shared_state.clone();
        thread::spawn(move || {
            thread::sleep(duration);
            println!("{:?} passed...", duration);
            let mut shared_state = thread_shared_state.lock().unwrap();
            shared_state.completed = true;
            if let Some(waker) = shared_state.waker.take() {
                waker.wake();
            }
        });

        TimeFuture { shared_state }
    }
}

struct Executor {
    ready_queue: Receiver<Arc<Task>>,
}

impl Executor {
    fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            let mut future_opt = task.future.lock().unwrap();
            if let Some(mut future) = future_opt.take() {
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&*waker);
                if future.as_mut().poll(context).is_pending() {
                    *future_opt = Some(future);
                }
            }
        }
    }
}

#[derive(Clone)]
struct Spwaner {
    task_sender: SyncSender<Arc<Task>>,
}

impl Spwaner {
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let task = Arc::new(
            Task {
                future: Mutex::new(Some(future.boxed())),
                task_sender: self.task_sender.clone(),
            }
        );
        self.task_sender.send(task).expect("too many tasks queued");
    }
}

struct Task {
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    task_sender: SyncSender<Arc<Task>>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let clone_task = arc_self.clone();
        arc_self.task_sender.send(clone_task).expect("too many task queued");
    }
}

fn new_executor_and_spawner() -> (Executor, Spwaner) {
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (task_sender, ready_queue) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { ready_queue }, Spwaner { task_sender })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let (executor, spawner) = new_executor_and_spawner();
        spawner.spawn(async {
            TimeFuture::new(Duration::new(2, 0)).await;
        });
        drop(spawner);
        executor.run();
        println!("all task finish...");
    }
}
