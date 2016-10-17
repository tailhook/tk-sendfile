extern crate futures;
extern crate tokio_core;
extern crate tk_sendfile;

use std::net::SocketAddr;
use std::env;

use futures::Future;
use futures::stream::Stream;
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tk_sendfile::DiskPool;


fn main() {
    let addr = env::args().nth(1).unwrap_or("127.0.0.1:7777".to_string());
    let addr = addr.parse::<SocketAddr>().unwrap();
    let disk_pool = DiskPool::new();

    let mut lp = Core::new().unwrap();
    let handle = lp.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Listening on: {}", addr);

    let done = socket.incoming().for_each(|(socket, _addr)| {
        handle.spawn(
            disk_pool.send("./examples/send_myself.rs", socket)
            .then(|result| {
                println!("Result {:?}", result);
                Ok(())
            })
        );
        Ok(())
    });

    lp.run(done).unwrap();
}
