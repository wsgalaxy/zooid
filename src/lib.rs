use std::collections::HashMap;
use std::{future::Future, marker::PhantomData};
use tokio::sync::{mpsc, oneshot};

pub struct SendError<T>(T);
pub enum TrySendError<T> {
    Full(T),
    Closed(T),
}

pub struct Addr<A>
where
    A: Actor,
{
    tx: mpsc::Sender<Envelope<<A as Actor>::Msg, <A as Actor>::Rsp>>,
}

impl<A> Addr<A>
where
    A: Actor,
{
    /// Send message to Actor and wait for response.
    pub async fn send(&self, msg: A::Msg) -> Result<Option<A::Rsp>, SendError<A::Msg>> {
        let (tx, rx) = oneshot::channel();
        // Send to actor
        if let Err(mpsc::error::SendError(evlp)) = self
            .tx
            .send(Envelope {
                msg,
                rsp_tx: Some(tx),
            })
            .await
        {
            return Err(SendError(evlp.msg));
        }
        // Now wait for response
        match rx.await {
            Ok(rsp) => Ok(Some(rsp)),
            Err(_) => Ok(None),
        }
    }

    /// Try to send a message to Actor and not wait for the response, fail if the
    /// channel is full or the actor is stopped.
    pub fn try_send(&self, msg: A::Msg) -> Result<(), TrySendError<A::Msg>> {
        if let Err(e) = self.tx.try_send(Envelope { msg, rsp_tx: None }) {
            match e {
                mpsc::error::TrySendError::Full(evlp) => Err(TrySendError::Full(evlp.msg)),
                mpsc::error::TrySendError::Closed(evlp) => Err(TrySendError::Closed(evlp.msg)),
            }
        } else {
            Ok(())
        }
    }
}

impl<A> Clone for Addr<A>
where
    A: Actor,
{
    fn clone(&self) -> Self {
        Addr {
            tx: self.tx.clone(),
        }
    }
}

pub trait Message: Send + 'static {}
pub trait Response: Send + 'static {}

impl<T> Message for T where T: Send + Sized + 'static {}
impl<T> Response for T where T: Send + Sized + 'static {}

struct Envelope<M, R>
where
    M: Message,
    R: Response,
{
    msg: M,
    rsp_tx: Option<oneshot::Sender<R>>,
}

pub struct Context<A>
where
    A: Actor,
{
    pack_tx: mpsc::Sender<Box<dyn Package<A>>>,
    pack_rx: Option<mpsc::Receiver<Box<dyn Package<A>>>>,
    task_handles: HashMap<usize, Box<dyn AsyncTaskHandle>>,
    _p1: PhantomData<A>,
}

impl<A> Context<A>
where
    A: Actor,
{
    fn new() -> Self {
        // TODO: Support config channel size
        let (pack_tx, pack_rx) = mpsc::channel(64);
        Context {
            pack_tx,
            pack_rx: Some(pack_rx),
            task_handles: HashMap::new(),
            _p1: Default::default(),
        }
    }

    async fn run(
        mut self,
        mut act: A,
        addr: Addr<A>,
        mut rx: mpsc::Receiver<Envelope<A::Msg, A::Rsp>>,
    ) {
        act.start(&mut self, addr);
        let mut pack_rx = self.pack_rx.take().unwrap();
        let mut pack_rx_closed = false;
        loop {
            tokio::select! {
                // Receive message from Addr and make the message be handled by actor
                evlp = rx.recv() => {
                    if let Some(evlp) = evlp {
                        act.handle_msg_async(&mut self, evlp.msg, evlp.rsp_tx);
                    } else {
                        // All Addr dropped, this actor is going to die.
                        break;
                    }
                }
                // Receive package and unpack it
                pack = pack_rx.recv(), if !pack_rx_closed => {
                    if let Some(mut pack) = pack {
                        pack.unpack(&mut self, &mut act);
                    } else {
                        pack_rx_closed = true;
                    }
                }
            }
        }
        // Cancel all async tasks before we stop
        self.clear_async_tasks();
        // Now stop Actor
        act.stop(&mut self);
    }

    pub fn spawn<Fut, F>(&mut self, fut: Fut, f: F)
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
        F: FnOnce(&mut Context<A>, &mut A, <Fut as Future>::Output) + Send + 'static,
    {
        // Request an id and if failed, just panic
        let id = self.gen_id();
        let pack_tx = self.pack_tx.clone();
        let jh = tokio::spawn(async move {
            let _ = pack_tx
                .send(Box::new(FuturePackage {
                    fut_res: Some(fut.await),
                    callback: Some(f),
                    id,
                    _p1: Default::default(),
                }))
                .await;
        });
        // Register this async task so we get the chance to cancel it
        self.task_handles
            .insert(id, Box::new(FutureHandle { jh: Some(jh) }));
    }

    fn clear_async_tasks(&mut self) {
        for (_, v) in &mut self.task_handles {
            v.cancel();
        }
        self.task_handles.clear();
    }

    fn gen_id(&mut self) -> usize {
        let mut id = rand::random();
        while self.task_handles.contains_key(&id) {
            id = rand::random();
        }
        id
    }
}

pub struct ContextBuilder<A>
where
    A: Actor,
{
    rx: mpsc::Receiver<Envelope<<A as Actor>::Msg, <A as Actor>::Rsp>>,
    tx: mpsc::Sender<Envelope<<A as Actor>::Msg, <A as Actor>::Rsp>>,
}

impl<A> ContextBuilder<A>
where
    A: Actor,
{
    pub fn new(cap: usize) -> Self {
        let (tx, rx) = mpsc::channel(cap);
        Self { rx, tx }
    }

    pub fn start(self, act: A) -> Addr<A> {
        let addr = Addr { tx: self.tx };
        let _addr = addr.clone();
        let ctx = Context::new();
        tokio::spawn(ctx.run(act, _addr, self.rx));
        addr
    }

    pub fn addr(&self) -> Addr<A> {
        Addr {
            tx: self.tx.clone(),
        }
    }
}

pub trait Actor: Send + Sized + 'static {
    type Msg: Message;
    type Rsp: Response;

    fn start(&mut self, _ctx: &mut Context<Self>, _addr: Addr<Self>) {}
    fn stop(&mut self, _ctx: &mut Context<Self>) {}

    fn handle_msg(&mut self, _ctx: &mut Context<Self>, _msg: Self::Msg) -> Option<Self::Rsp> {
        None
    }

    /// Rewrite this function to do async msg handling
    fn handle_msg_async(
        &mut self,
        _ctx: &mut Context<Self>,
        _msg: Self::Msg,
        rsp_tx: Option<oneshot::Sender<Self::Rsp>>,
    ) {
        let rsp = self.handle_msg(_ctx, _msg);
        if let None = rsp {
            return;
        }
        if let Some(tx) = rsp_tx {
            let _ = tx.send(rsp.unwrap());
        }
    }
}

trait Package<A>: Send + 'static
where
    A: Actor,
{
    fn unpack(&mut self, ctx: &mut Context<A>, act: &mut A);
}

struct FuturePackage<A, FutRes, F>
where
    A: Actor,
    FutRes: Send + 'static,
    F: FnOnce(&mut Context<A>, &mut A, FutRes) + Send + 'static,
{
    fut_res: Option<FutRes>,
    callback: Option<F>,
    id: usize,
    _p1: PhantomData<A>,
}

impl<A, FutRes, F> Package<A> for FuturePackage<A, FutRes, F>
where
    A: Actor,
    FutRes: Send + 'static,
    F: FnOnce(&mut Context<A>, &mut A, FutRes) + Send + 'static,
{
    fn unpack(&mut self, ctx: &mut Context<A>, act: &mut A) {
        // Remove JoinHandle
        ctx.task_handles.remove(&self.id);

        let callback = self.callback.take().unwrap();
        let fut_res = self.fut_res.take().unwrap();
        callback(ctx, act, fut_res);
    }
}

trait AsyncTaskHandle: Send + 'static {
    fn cancel(&mut self);
}

struct FutureHandle<T>
where
    T: Send + 'static,
{
    jh: Option<tokio::task::JoinHandle<T>>,
}

impl<T> AsyncTaskHandle for FutureHandle<T>
where
    T: Send + 'static,
{
    fn cancel(&mut self) {
        self.jh.take().unwrap().abort();
    }
}
