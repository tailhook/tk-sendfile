extern crate futures;
extern crate argparse;
extern crate tokio_core;
extern crate tk_sendfile;

use std::net::SocketAddr;
use std::path::PathBuf;

use argparse::{ArgumentParser, Parse};
use futures::Future;
use futures::stream::Stream;
use tokio_core::net::TcpListener;
use tokio_core::io::write_all;
use tokio_core::reactor::Core;
use tk_sendfile::DiskPool;


fn main() {
    let mut filename = PathBuf::from("examples/pseudo_http.rs");
    let mut addr = "127.0.0.1:8080".parse::<SocketAddr>().unwrap();
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Serve a file via HTTP on any open connection");
        ap.refer(&mut addr)
           .add_option(&["-l", "--listen"], Parse,
            "Listening address");
        ap.refer(&mut filename)
           .add_option(&["-f", "--filename"], Parse,
            "File to serve");
        ap.parse_args_or_exit();
    }
    let disk_pool = DiskPool::new();

    let mut lp = Core::new().unwrap();
    let handle = lp.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Listening on: {} ({:?})", addr, filename);

    let done = socket.incoming().for_each(|(socket, _addr)| {
        handle.spawn(
            disk_pool.open(filename.clone())
            .then(|file| {
                let header = format!("HTTP/1.0 200 OK\r\n\
                    Content-Length: {}\r\n\
                    \r\n", file.size());
                write_all(socket, header)
                .and_then(|(socket, _)| file.write_into(socket))
            })
            .then(|result| {
                println!("Result {:?}", result);
                Ok(())
            })
        );
        Ok(())
    });

    lp.run(done).unwrap();
}
