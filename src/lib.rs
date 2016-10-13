use std::io;
use std::fs::File;
use std::path::PathBuf;
use std::os::unix::io::AsRawFd;


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
    fn open(&mut self) -> Result<&AsRawFd, io::Error>;
}

/// Trait that represents something that can be converted into a file
/// FileOpener
///
/// This is very similar to IntoIterator or IntoFuture and used in similar
/// way.
///
/// Note unlike methods in FileOpener itself this trait is executed in
/// caller thread, **not** in disk thread.
pub trait IntoFileOpener {
    type Opener;
    fn into_file_opener(self) -> Self::Opener;
}

/// File opener implementation that opens specified file path directly
#[derive(Debug)]
pub struct PathOpener(PathBuf, Option<File>);

impl<T: Into<PathBuf>> IntoFileOpener for T {
    type Opener = PathOpener;
    fn into_file_opener(self) -> PathOpener {
        PathOpener(self.into(), None)
    }
}

impl FileOpener for PathOpener {
    fn open(&mut self) -> Result<&AsRawFd, io::Error> {
        if self.1.is_none() {
            self.1 = Some(try!(File::open(&self.0)));
        }
        Ok(self.1.as_ref().unwrap())
    }
}
