Sendfile for Tokio
==================

:Status: alpha
:Documentation: http://docs.rs/tk-sendfile

A thread pool that can process file requests and send data to the socket
with zero copy (using sendfile).

Challenges:

1. While ``sendfile`` is non-blocking when writing to socket, it blocks for
   disk access for two cases: initial ``open()`` and for reading file inside
   the ``sendfile()`` system call itself
2. Doing more than single ``sendfile`` operation requires some bookkeeping
3. [TODO] Serving user-specified file paths requires non-trivial code to
   prevent symlink attacks


License
=======

Licensed under either of

* Apache License, Version 2.0,
  (./LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license (./LICENSE-MIT or http://opensource.org/licenses/MIT)
  at your option.

Contribution
------------

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

