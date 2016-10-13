//! A thread pool that can process file requests and send data to the socket
//! with zero copy (using sendfile).
//!
//! Use `DiskPool` structure to request file operations.
#![deny(missing_docs)]

extern crate libc;
extern crate futures;
extern crate tokio_core;
extern crate futures_cpupool;

use std::io;
use std::fs::File;
use std::sync::Arc;
use std::path::PathBuf;
use std::os::unix::io::AsRawFd;

use libc::sendfile;
use futures_cpupool::CpuPool;

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
pub trait Destination: Send {
    /// Write data to the file
    fn write(&mut self, file: &mut Sendfile) -> Result<usize, io::Error>;
}

/// A structure that tracks progress of sending a file
pub struct Sendfile {
    file: Box<FileOpener>,
    offset: u64,
}

/// Future that is returned from `DiskPool::send`
type SendfileFuture<D> = futures_cpupool::CpuFuture<D, io::Error>;

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
    pub fn send<F, D>(&self, file: F, destination: D)
        -> SendfileFuture<D>
        where F: IntoFileOpener + Send + Sized + 'static,
              D: Destination + Send + Sized + 'static,
    {
        let file = Sendfile {
            file: Box::new(file.into_file_opener()),
            offset: 0,
        };
        self.0.pool.spawn_fn(move || {
            read_file(file, destination)
        })
    }
}

impl Destination for tokio_core::net::TcpStream {
    fn write(&mut self, file: &mut Sendfile) -> Result<usize, io::Error> {
        let (file_ref, size) = try!(file.file.open());
        let mut offset = file.offset as i64;
        let rc = unsafe {
            sendfile(self.as_raw_fd(), file_ref.as_raw_fd(),
                     &mut offset,
                     size.saturating_sub(file.offset) as usize)
        };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else if rc == 0 {
            Err(io::Error::new(io::ErrorKind::WriteZero,
                           "connection closed while sending file"))
        } else {
            Ok(rc as usize)
        }
    }
}

fn read_file<D>(mut file: Sendfile, mut dest: D)
    -> Result<D, io::Error>
    where D: Destination
{
    dest.write(&mut file)
    .map(|bytes_sent| file.offset += bytes_sent as u64)
    .map(|()| dest)
}
