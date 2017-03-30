extern crate futures;
extern crate futures_cpupool;
extern crate argparse;
extern crate tokio_core;
extern crate tokio_io;
extern crate tk_sendfile;

use std::net::SocketAddr;
use std::path::PathBuf;

use argparse::{ArgumentParser, Parse};
use futures::Future;
use futures::stream::Stream;
use futures_cpupool::CpuPool;
use tokio_core::net::TcpListener;
use tokio_io::io::{write_all, read_to_end};
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
    // While it's not interesting in demo, it's recommented to have fairly
    // large number of threads here for three reasons:
    //
    // 1. To make use of device parallelism
    // 2. To allow kernel to merge some disk operations
    // 3. To fix head of line blocking when some request reach disk but others
    //    could be served immediately from cache (we don't know which are
    //    cached)
    let disk_pool = DiskPool::new(CpuPool::new(40));

    let mut lp = Core::new().unwrap();
    let handle = lp.handle();
    let socket = TcpListener::bind(&addr, &handle).unwrap();
    println!("Listening on: {} ({:?})", addr, filename);

    let done = socket.incoming().for_each(|(socket, _addr)| {
        handle.spawn(
            disk_pool.open(filename.clone())
            .and_then(|file| {
                let header = format!("HTTP/1.0 200 OK\r\n\
                    Content-Length: {}\r\n\
                    Connection: close\r\n\
                    \r\n", file.size());
                write_all(socket, header)
                .and_then(|(socket, _)| file.write_into(socket))
                // Wait until client closes connection
                // This is needed to ensure that peer has received data
                // (otherwise close() looses some data)
                .and_then(|socket| read_to_end(socket, Vec::new()))
            })
            .then(|_result| {
                //println!("Done {:?}",
                //    _result.map(|(_, b)| format!("{} bytes read", b.len())));
                Ok(())
            })
        );
        Ok(())
    });

    lp.run(done).unwrap();
}
