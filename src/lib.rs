use std::marker::PhantomData;
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
    pub fn try_send() -> Result<(), TrySendError<A::Rsp>> {
        // TODO
        Ok(())
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

pub trait Message: Send + Sized + 'static {}
pub trait Response: Send + Sized + 'static {}

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
    _p1: PhantomData<A>,
}

impl<A> Context<A>
where
    A: Actor,
{
    fn new() -> Self {
        Context {
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
        loop {
            match rx.recv().await {
                Some(evlp) => {
                    act.handle_msg_async(&mut self, evlp.msg, evlp.rsp_tx);
                }
                None => break,
            }
        }
        act.stop(&mut self);
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
