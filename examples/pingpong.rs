use zooid::{Actor, Context, ContextBuilder};

struct MyActor;
impl Actor for MyActor {
    type Msg = ();
    type Rsp = ();

    fn handle_msg(&mut self, _ctx: &mut Context<Self>, _msg: Self::Msg) -> Option<Self::Rsp> {
        println!("handle msg");
        None
    }
}

#[tokio::main]
async fn main() {
    let ctx_builder = ContextBuilder::<MyActor>::new(1024);
    let act = MyActor;
    let addr = ctx_builder.start(act);
    loop {
        if let Err(_) = addr.send(()).await {
            println!("actor dead!");
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
