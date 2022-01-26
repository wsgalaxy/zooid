use tokio::time::Instant;
use zooid::{Actor, Addr, Context, ContextBuilder};

struct Ping {
    recorder: Addr<Recorder>,
    pong: Addr<Pong>,
}

impl Actor for Ping {
    type Msg = ();
    type Rsp = ();

    fn start(&mut self, _ctx: &mut Context<Self>, _addr: zooid::Addr<Self>) {
        let recorder = self.recorder.clone();
        let pong = self.pong.clone();
        _ctx.spawn(async move {
            let sleep = tokio::time::sleep(tokio::time::Duration::from_millis(200));
            tokio::pin!(sleep);
            let mut count: usize = 0;
            loop {
                tokio::select! {
                    rsp = pong.send(()) => {
                        if let Err(_) = rsp {
                            println!("Peer dead!");
                            break;
                        } else {
                            count += 1;
                        }
                    }

                    _ = &mut sleep => {
                        let _ = recorder.send(count).await;
                        count = 0;
                        sleep.as_mut().reset(Instant::now() + tokio::time::Duration::from_millis(200));
                    }
                }
            }
        }, |_, _, _| {});
    }

    fn stop(&mut self, _ctx: &mut Context<Self>) {
        println!("Ping stopped");
    }
}

struct Pong;

impl Actor for Pong {
    type Msg = ();
    type Rsp = ();

    fn handle_msg(&mut self, _ctx: &mut Context<Self>, _msg: Self::Msg) -> Option<Self::Rsp> {
        Some(())
    }

    fn stop(&mut self, _ctx: &mut Context<Self>) {
        println!("Pong stopped");
    }
}

struct Recorder {
    count: usize,
}

impl Recorder {
    fn dump_statistics(_ctx: &mut Context<Self>, _act: &mut Self, _: ()) {
        println!("{}/s", _act.count);
        _act.count = 0;
        _ctx.spawn(
            async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            },
            Recorder::dump_statistics,
        );
    }
}

impl Actor for Recorder {
    type Msg = usize;
    type Rsp = ();

    fn start(&mut self, _ctx: &mut Context<Self>, _addr: Addr<Self>) {
        Recorder::dump_statistics(_ctx, self, ());
    }

    fn handle_msg(&mut self, _ctx: &mut Context<Self>, _msg: Self::Msg) -> Option<Self::Rsp> {
        self.count += _msg;
        None
    }

    fn stop(&mut self, _ctx: &mut Context<Self>) {
        println!("Recorder sotpped");
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args();
    let mut cocurrency = num_cpus::get();

    if args.len() >= 2 {
        if let Ok(n) = args.nth(1).unwrap().parse() {
            cocurrency = n;
        }
    }

    println!("Use cocurrency {}", cocurrency);

    let addr_recorder = ContextBuilder::new(1024).start(Recorder { count: 0 });
    let mut addr_ping_vec = Vec::new();

    for _ in 0..cocurrency {
        addr_ping_vec.push(ContextBuilder::new(1024).start(Ping {
            recorder: addr_recorder.clone(),
            pong: ContextBuilder::new(1024).start(Pong),
        }));
    }

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
