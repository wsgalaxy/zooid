use zooid::{Actor, Context, Handler};

static mut COUNTER: u64 = 0;

struct MyActor {}

impl Actor for MyActor {}

impl Handler<()> for MyActor {
    type Result = ();
    fn handle(&mut self, _ctx: &mut Context<Self>, _m: ()) -> Self::Result {
        ()
    }
}

#[tokio::main]
async fn main() {
    let a = MyActor {};
    let ctx = Context::new();
    let addr = ctx.start(a);

    tokio::spawn(async {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let c = unsafe {
                let c = COUNTER;
                COUNTER = 0;
                c
            };
            println!("{} msg/s", c);
        }
    });

    loop {
        let _ = addr.send(()).await;
        unsafe {
            COUNTER += 1;
        }
    }
}
