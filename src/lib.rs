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
pub trait FileOpener: Debug {
    fn open(&mut self) -> Result<Box<AsRawFd>, io::Error>;
}

impl<T: AsRef<Path> + Debug> FileOpener for T {
    fn open(&mut self) -> Result<Box<AsRawFd>, io::Error> {
        Ok(Box::new(try!(File::open(self.as_ref()))))
    }
}
