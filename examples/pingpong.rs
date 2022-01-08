use zooid::{Actor, Context, Handler};

struct Ping {}

impl Actor for Ping {}

impl Handler<i32> for Ping {
    type Result = i32;

    fn handle(&mut self, _ctx: &mut Context<Self>, m: i32) -> Self::Result {
        println!("Ping got {}", m);
        m + 1
    }
}

#[tokio::main]
async fn main() {
    let act = Ping {};
    let ctx = Context::<Ping>::new();
    let addr = ctx.addr();
    ctx.start(act);

    let mut i = 0;
    loop {
        if let Ok(r) = addr.send(i).await {
            println!("Receive {}", r);
            i += 1;
        } else {
            panic!("Actor dead!");
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
