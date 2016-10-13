use std::io;
use std::fs::File;
use std::fmt::Debug;
use std::path::Path;
use std::os::unix::io::AsRawFd;


/// This trait represents anything that can open the file
///
/// It's implemented for `AsRef<Path>` to just open a file at specified path.
/// But often you want some checks of permissions or visibility of the file
/// while opening it. You can't do even `stat()` or `open()` in main loop
/// because even such a simple operation can potentially block for indefinite
/// period of time.
///
/// So file opener could be anything that validates a path,
/// caches file descriptor, and in the result returns a file.
pub trait FileOpener: Debug + Send {
    /// Read file from cache
    ///
    /// Note: this can be both positive and negative cache
    ///
    /// You don't have to implement this method if you don't have in-memory
    /// cache of files
    fn from_cache(&mut self) -> Option<Result<Box<AsRef<[u8]>>, io::Error>> {
        None
    }
    /// Open the file
    ///
    /// This function is called in disk thread
    fn open(&mut self) -> Result<Box<AsRawFd>, io::Error>;
}

impl<T: AsRef<Path> + Debug + Send> FileOpener for T {
    fn open(&mut self) -> Result<Box<AsRawFd>, io::Error> {
        Ok(Box::new(try!(File::open(self.as_ref()))))
    }
}
