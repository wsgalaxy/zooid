use std::any::Any;
use tokio::sync::{mpsc, oneshot};

/// An Actor that run by Context<A>
pub trait Actor: Sized + Send + 'static {}

enum ContextMsg<A>
where
    A: Actor,
{
    CloneTx,
    DropTx,
    Msg(Box<dyn EnvelopeProxy<A>>),
}

impl<A> ContextMsg<A>
where
    A: Actor,
{
    fn into_msg<M>(self) -> Option<M>
    where
        M: Message,
    {
        if let ContextMsg::Msg(mut m) = self {
            if let Some(env) = m.as_any_mut().downcast_mut::<Envelope<M>>() {
                env.msg.take()
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Execution context for actor
pub struct Context<A>
where
    A: Actor,
{
    tx: Option<mpsc::UnboundedSender<ContextMsg<A>>>,
    tx_count: u64,
    rx: Option<mpsc::UnboundedReceiver<ContextMsg<A>>>,
}

impl<A: Actor> Context<A> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Context {
            tx: Some(tx),
            tx_count: 0,
            rx: Some(rx),
        }
    }

    pub fn start(self, act: A) -> Addr<A> {
        let addr = self.addr();
        tokio::spawn(self.spawn(act));
        addr
    }

    pub fn addr(&self) -> Addr<A> {
        Addr {
            tx: self.tx.clone(),
        }
    }

    async fn spawn(mut self, mut act: A) {
        if let None = self.rx {
            return;
        }

        let mut rx = self.rx.take().unwrap();

        loop {
            if let Some(msg) = rx.recv().await {
                match msg {
                    ContextMsg::CloneTx => self.tx_count += 1,
                    ContextMsg::DropTx => {
                        self.tx_count -= 1;
                        if self.tx_count == 0 {
                            // All tx dropped
                            return;
                        }
                    }
                    ContextMsg::Msg(mut msg) => {
                        msg.handle(&mut self, &mut act);
                    }
                }
            } else {
                return;
            }
        }
    }
}

/// Message type
pub trait Message: Send + 'static {
    type Resp: Send + 'static;
}

/// Actor that could handle message type M should impl Handler<M>
pub trait Handler<M>
where
    Self: Actor,
    M: Message,
{
    fn handle(&mut self, ctx: &mut Context<Self>, m: M) -> M::Resp;
}

pub enum SendError<M> {
    Closed(M),
}

/// Address for Actor
pub struct Addr<A>
where
    A: Actor,
{
    tx: Option<mpsc::UnboundedSender<ContextMsg<A>>>,
}

impl<A> Addr<A>
where
    A: Actor,
{
    /// Send message to actor and ignore any result
    pub fn do_send<M>(&self, m: M)
    where
        M: Message,
        A: Handler<M>,
    {
        if let Some(ref tx) = self.tx {
            let msg = Box::new(Envelope {
                msg: Some(m),
                tx_back: None,
            });
            let _ = tx.send(ContextMsg::Msg(msg));
        }
    }

    /// Send Message to Actor and wait for resp
    pub async fn send<M>(&self, m: M) -> Result<M::Resp, SendError<M>>
    where
        M: Message,
        A: Handler<M>,
    {
        if let Some(ref tx) = self.tx {
            let (oneshot_tx, oneshot_rx) = oneshot::channel::<M::Resp>();
            let msg = Box::new(Envelope {
                msg: Some(m),
                tx_back: Some(oneshot_tx),
            });
            if let Err(e) = tx.send(ContextMsg::Msg(msg)) {
                Err(SendError::Closed(e.0.into_msg().unwrap()))
            } else {
                // If no one send us a result, that would be a bug.
                Ok(oneshot_rx.await.unwrap())
            }
        } else {
            Err(SendError::Closed(m))
        }
    }
}

impl<A> Clone for Addr<A>
where
    A: Actor,
{
    fn clone(&self) -> Self {
        let mut tx_dead = false;
        if let Some(ref tx) = self.tx {
            // Tell Context that we are cloning a tx, tx_count should be increased.
            if let Err(_) = tx.send(ContextMsg::CloneTx) {
                tx_dead = true;
            }
        }

        if tx_dead {
            // No need to clone tx
            Addr { tx: None }
        } else {
            Addr {
                tx: self.tx.clone(),
            }
        }
    }
}

impl<A> Drop for Addr<A>
where
    A: Actor,
{
    fn drop(&mut self) {
        if let Some(ref tx) = self.tx {
            let _ = tx.send(ContextMsg::DropTx);
        }
    }
}

trait EnvelopeProxy<A>: Send
where
    A: Actor,
{
    fn handle(&mut self, ctx: &mut Context<A>, act: &mut A);
}

struct Envelope<M>
where
    M: Message,
{
    msg: Option<M>,
    // Send the result back to receiver
    tx_back: Option<oneshot::Sender<M::Resp>>,
}

impl<A, M> EnvelopeProxy<A> for Envelope<M>
where
    A: Actor + Handler<M>,
    M: Message,
{
    fn handle(&mut self, ctx: &mut Context<A>, act: &mut A) {
        let resp = act.handle(ctx, self.msg.take().unwrap());
        if let Some(_) = self.tx_back {
            let tx_back = self.tx_back.take().unwrap();
            let _ = tx_back.send(resp);
        }
    }
}

trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T> AsAny for T
where
    T: Any,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
