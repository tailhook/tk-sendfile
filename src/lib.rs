//! A thread pool that can process file requests and send data to the socket
//! with zero copy (using sendfile).
//!
//! Use `DiskPool` structure to request file operations.
#![warn(missing_docs)]

extern crate nix;
extern crate futures;
extern crate tokio_core;
extern crate futures_cpupool;

use std::io;
use std::mem;
use std::fs::File;
use std::sync::Arc;
use std::path::PathBuf;
use std::os::unix::io::AsRawFd;

use futures::{Future, Poll, Async, BoxFuture, finished, failed};
use futures_cpupool::{CpuPool, CpuFuture};
use nix::sys::sendfile::sendfile;

/// A reference to a thread pool for disk operations
#[derive(Clone)]
pub struct DiskPool(Arc<Inner>);

/// This trait represents anything that can open the file
///
/// You can convert anything that is `AsRef<Path>` to this trait and
/// it will just open a file at specified path.
/// But often you want some checks of permissions or visibility of the file
/// while opening it. You can't do even `stat()` or `open()` in main loop
/// because even such a simple operation can potentially block for indefinite
/// period of time.
///
/// So file opener could be anything that validates a path,
/// caches file descriptor, and in the result returns a file.
pub trait FileOpener: Send + 'static {
    /// Read file from cache
    ///
    /// Note: this can be both positive and negative cache
    ///
    /// You don't have to implement this method if you don't have in-memory
    /// cache of files
    fn from_cache(&mut self) -> Option<Result<&[u8], io::Error>> {
        None
    }
    /// Open the file
    ///
    /// This function is called in disk thread
    fn open(&mut self) -> Result<(&AsRawFd, u64), io::Error>;
}

/// Trait that represents something that can be converted into a file
/// FileOpener
///
/// This is very similar to IntoIterator or IntoFuture and used in similar
/// way.
///
/// Note unlike methods in FileOpener itself this trait is executed in
/// caller thread, **not** in disk thread.
pub trait IntoFileOpener: Send {
    /// The final type returned after conversion
    type Opener: FileOpener + Send + 'static;
    /// Convert the type into a file opener
    fn into_file_opener(self) -> Self::Opener;
}

/// File opener implementation that opens specified file path directly
#[derive(Debug)]
pub struct PathOpener(PathBuf, Option<(File, u64)>);

/// A trait that represents anything that file can be sent to
///
/// The trait is implemented for TcpStream right away but you might want
/// to implement your own thing, for example to prepend the data with file
/// length header
pub trait Destination: io::Write + Send {

    /// This method does the actual sendfile call
    ///
    /// Note: this method is called in other thread
    fn write_file<O: FileOpener>(&mut self, file: &mut Sendfile<O>)
        -> Result<usize, io::Error>;

    /// Test to see if this object may be writable
    fn poll_write(&mut self) -> Async<()>;
}

/// A structure that tracks progress of sending a file
pub struct Sendfile<O: FileOpener + Send + 'static> {
    file: O,
    pool: DiskPool,
    cached: bool,
    offset: u64,
    size: u64,
}

/// Future that is returned from `DiskPool::send`
type SendfileFuture<D> = futures_cpupool::CpuFuture<D, io::Error>;

/// Future returned by `Sendfile::write_into()`
pub struct WriteFile<F: FileOpener, D: Destination>(DiskPool, WriteState<F, D>)
    where F: Send + 'static, D: Send + 'static;

enum WriteState<F: FileOpener, D: Destination> {
    Mem(Sendfile<F>, D),
    WaitSend(CpuFuture<(Sendfile<F>, D), io::Error>),
    WaitWrite(Sendfile<F>, D),
    Empty,
}

struct Inner {
    pool: CpuPool,
}

impl<T: Into<PathBuf> + Send> IntoFileOpener for T {
    type Opener = PathOpener;
    fn into_file_opener(self) -> PathOpener {
        PathOpener(self.into(), None)
    }
}

impl FileOpener for PathOpener {
    fn open(&mut self) -> Result<(&AsRawFd, u64), io::Error> {
        if self.1.is_none() {
            let file = try!(File::open(&self.0));
            let meta = try!(file.metadata());
            self.1 = Some((file, meta.len()));
        }
        Ok(self.1.as_ref().map(|&(ref f, s)| (f as &AsRawFd, s)).unwrap())
    }
}

impl DiskPool {
    /// Create a disk pool with default configuration
    pub fn new() -> DiskPool {
        DiskPool(Arc::new(Inner {
            pool: CpuPool::new(20),
        }))
    }
    /// Start a file send operation
    pub fn open<F>(&self, file: F)
        // TODO(tailhook) unbox a future
        -> BoxFuture<Sendfile<F::Opener>, io::Error>
        where F: IntoFileOpener + Send + Sized + 'static,
    {
        let mut file = file.into_file_opener();
        let cached_size = match file.from_cache() {
            Some(Ok(cache_ref)) => {
                Some(cache_ref.len() as u64)
            }
            Some(Err(e)) => {
                return failed(e).boxed();
            }
            None => None,
        };
        let pool = self.clone();
        if let Some(size) = cached_size {
            finished(Sendfile {
                file: file,
                pool: pool,
                cached: true,
                offset: 0,
                size: size,
            }).boxed()
        } else {
            self.0.pool.spawn_fn(move || {
                let (_, size) = try!(file.open());
                let file = Sendfile {
                    file: file,
                    pool: pool,
                    cached: false,
                    offset: 0,
                    size: size,
                };
                Ok(file)
            }).boxed()
        }
    }
    /// A shortcut method to send whole file without headers
    pub fn send<F, D>(&self, file: F, destination: D)
        -> futures::BoxFuture<D, io::Error>
        where F: IntoFileOpener + Send + Sized + 'static,
              D: Destination + Send + Sized + 'static,
    {
        self.open(file).and_then(|file| file.write_into(destination)).boxed()
    }
}

impl Destination for tokio_core::net::TcpStream {
    fn write_file<O: FileOpener>(&mut self, file: &mut Sendfile<O>)
        -> Result<usize, io::Error>
    {
        let (file_ref, size) = try!(file.file.open());
        let mut offset = file.offset as i64;
        let result = sendfile(self.as_raw_fd(), file_ref.as_raw_fd(),
                         Some(&mut offset),
                         size.saturating_sub(file.offset) as usize);
        match result {
            Ok(x) => Ok(x),
            Err(nix::Error::Sys(x)) => {
                Err(io::Error::from_raw_os_error(x as i32))
            }
            Err(nix::Error::InvalidPath) => unreachable!(),
        }
    }
    fn poll_write(&mut self) -> Async<()> {
        use nix::poll::{poll, PollFd, EventFlags, POLLOUT };

        let mut fd = [PollFd::new(self.as_raw_fd(), POLLOUT,
                                  EventFlags::empty())];
        if let Ok(1) = poll(&mut fd, 0) {
            return Async::Ready(());
        } else {
            return Async::NotReady;
        }
        // This doesn't work well, it returns Ready every time
        // tokio_core::net::TcpStream::poll_write(self)
    }
}

impl<O: FileOpener> Sendfile<O> {
    /// Returns full size of the file
    ///
    /// Note that if file changes while we are reading it, we may not be
    /// able to send this number of bytes. In this case we will return
    /// `WriteZero` error however.
    pub fn size(&self) -> u64 {
        return self.size;
    }
    /// Returns a future which resolves to original socket when file has been
    /// written into a file
    pub fn write_into<D: Destination>(self, dest: D) -> WriteFile<O, D> {
        if self.cached {
            WriteFile(self.pool.clone(), WriteState::Mem(self, dest))
        } else {
            WriteFile(self.pool.clone(), WriteState::WaitWrite(self, dest))
        }
    }
}

impl<F: FileOpener, D: Destination> Future for WriteFile<F, D>
    where F: Send + 'static, D: Send + 'static,
{
    type Item = D;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<D, io::Error> {
        use self::WriteState::*;
        loop {
            self.1 = match mem::replace(&mut self.1, Empty) {
                Mem(mut file, mut dest) => {
                    let need_switch = match file.file.from_cache() {
                        Some(Ok(slice)) => {
                            if (slice.len() as u64) < file.size {
                                return Err(io::Error::new(
                                    io::ErrorKind::WriteZero,
                                    "cached file truncated during writing"));
                            }
                            let target_slice = &slice[file.offset as usize..];
                            // Not sure why we can reach it, but it's safe
                            if target_slice.len() == 0 {
                                return Ok(Async::Ready(dest));
                            }
                            match dest.write(target_slice) {
                                Ok(0) => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::WriteZero,
                                        "connection closed while sending \
                                         file from cache"));
                                }
                                Ok(bytes) => {
                                    file.offset += bytes as u64;
                                    if file.offset >= file.size {
                                        return Ok(Async::Ready(dest));
                                    }
                                }
                                Err(e) => {
                                    return Err(e);
                                }
                            }
                            false
                        }
                        Some(Err(e)) => {
                            return Err(e);
                        }
                        None => {
                            // File evicted from cache in the middle of sending
                            // Switch to non-cached variant
                            // TODO(tailhook) should we log it?
                            true
                        }
                    };
                    if need_switch {
                        WaitWrite(file, dest)
                    } else {
                        Mem(file, dest)
                    }
                }
                WaitSend(mut future) => {
                    match future.poll() {
                        Ok(Async::Ready((file, dest))) => {
                            if file.size == file.offset {
                                return Ok(Async::Ready(dest));
                            } else {
                                WaitWrite(file, dest)
                            }
                        }
                        Ok(Async::NotReady) => WaitSend(future),
                        Err(e) => return Err(e),
                    }
                }
                WaitWrite(mut file, mut dest) => {
                    match dest.poll_write() {
                        Async::Ready(()) => {
                            WaitSend((self.0).0.pool.spawn_fn(move || {
                                match dest.write_file(&mut file) {
                                    Ok(0) => {
                                        Err(io::Error::new(
                                            io::ErrorKind::WriteZero,
                                            "connection closed while \
                                             sending a file"))
                                    }
                                    Ok(bytes_sent) => {
                                        file.offset += bytes_sent as u64;
                                        Ok((file, dest))
                                    }
                                    Err(ref e)
                                    if e.kind() == io::ErrorKind::WouldBlock
                                    => {
                                        Ok((file, dest))
                                    }
                                    Err(e) => Err(e),
                                }
                            }))
                        }
                        Async::NotReady => WaitWrite(file, dest),
                    }
                }
                Empty => unreachable!(),
            }
        }
    }
}
