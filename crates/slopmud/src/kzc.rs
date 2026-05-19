use std::io;
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

const FALLBACK_CHUNK_BYTES: usize = 1024 * 1024;

#[cfg(target_os = "linux")]
pub async fn send_file_to_writer(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    path: &Path,
    len: u64,
) -> io::Result<u64> {
    use std::os::fd::AsRawFd;

    let socket_fd = writer.as_ref().as_raw_fd();
    let path_owned = path.to_owned();
    let path_for_task = path_owned.clone();
    match tokio::task::spawn_blocking(move || {
        linux_io_uring_splice_file(socket_fd, &path_for_task, len)
    })
    .await
    {
        Ok(Ok(sent)) => Ok(sent),
        Ok(Err(e)) if zero_copy_setup_failed(&e) => {
            buffered_send_file(writer, &path_owned, len).await
        }
        Ok(Err(e)) => Err(e),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("zero-copy send task failed: {e}"),
        )),
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn send_file_to_writer(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    path: &Path,
    len: u64,
) -> io::Result<u64> {
    buffered_send_file(writer, path, len).await
}

async fn buffered_send_file<W>(writer: &mut W, path: &Path, len: u64) -> io::Result<u64>
where
    W: AsyncWrite + Unpin,
{
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; FALLBACK_CHUNK_BYTES];
    let mut remaining = len;
    let mut sent = 0u64;
    while remaining > 0 {
        let limit = remaining.min(buf.len() as u64) as usize;
        let n = file.read(&mut buf[..limit]).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        sent = sent.saturating_add(n as u64);
        remaining = remaining.saturating_sub(n as u64);
    }
    Ok(sent)
}

#[cfg(target_os = "linux")]
fn zero_copy_setup_failed(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::ENOSYS | libc::EPERM | libc::EINVAL | libc::EMFILE | libc::ENFILE)
    )
}

#[cfg(target_os = "linux")]
fn linux_io_uring_splice_file(
    socket_fd: std::os::fd::RawFd,
    path: &Path,
    len: u64,
) -> io::Result<u64> {
    use io_uring::{IoUring, opcode, types};
    use std::fs::File;
    use std::os::fd::{AsRawFd, RawFd};

    struct Pipe {
        read: RawFd,
        write: RawFd,
    }

    impl Drop for Pipe {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.read);
                libc::close(self.write);
            }
        }
    }

    fn pipe() -> io::Result<Pipe> {
        let mut fds = [0; 2];
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Pipe {
            read: fds[0],
            write: fds[1],
        })
    }

    fn submit_splice(
        ring: &mut IoUring,
        fd_in: RawFd,
        off_in: i64,
        fd_out: RawFd,
        off_out: i64,
        len: u32,
    ) -> io::Result<usize> {
        let entry =
            opcode::Splice::new(types::Fd(fd_in), off_in, types::Fd(fd_out), off_out, len).build();
        {
            let mut sq = ring.submission();
            unsafe {
                sq.push(&entry)
                    .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "io_uring sq full"))?;
            }
        }
        ring.submit_and_wait(1)?;
        let cqe = ring
            .completion()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "missing io_uring cqe"))?;
        let result = cqe.result();
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        Ok(result as usize)
    }

    let file = File::open(path)?;
    let pipe = pipe()?;
    let mut ring = IoUring::new(8)?;
    let mut offset: i64 = 0;
    let mut remaining = len;
    let mut sent = 0u64;

    while remaining > 0 {
        let request = remaining.min(FALLBACK_CHUNK_BYTES as u64) as u32;
        let mut from_file = loop {
            match submit_splice(&mut ring, file.as_raw_fd(), offset, pipe.write, -1, request) {
                Ok(0) => return Ok(sent),
                Ok(n) => break n,
                Err(e) if is_retryable_splice_error(&e) => {
                    std::thread::yield_now();
                }
                Err(e) => return Err(e),
            }
        };

        while from_file > 0 {
            let moved = loop {
                match submit_splice(
                    &mut ring,
                    pipe.read,
                    -1,
                    socket_fd,
                    -1,
                    from_file.min(u32::MAX as usize) as u32,
                ) {
                    Ok(0) => return Ok(sent),
                    Ok(n) => break n,
                    Err(e) if is_retryable_splice_error(&e) => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(e) => return Err(e),
                }
            };
            from_file -= moved;
            sent = sent.saturating_add(moved as u64);
            remaining = remaining.saturating_sub(moved as u64);
            offset = offset.saturating_add(moved as i64);
        }
    }

    Ok(sent)
}

#[cfg(target_os = "linux")]
fn is_retryable_splice_error(e: &io::Error) -> bool {
    let Some(code) = e.raw_os_error() else {
        return false;
    };
    code == libc::EAGAIN || code == libc::EWOULDBLOCK || code == libc::EINTR
}

#[cfg(test)]
mod tests {
    use super::send_file_to_writer;
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn sends_file_to_tcp_writer() {
        let payload = (0..128 * 1024).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        let path = std::env::temp_dir().join(format!(
            "slopmud-kzc-{}-{}.blob",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::write(&path, &payload).await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let send_path = path.clone();
        let len = payload.len() as u64;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (_rd, mut wr) = stream.into_split();
            let sent = send_file_to_writer(&mut wr, &send_path, len).await.unwrap();
            assert_eq!(sent, len);
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut got = vec![0u8; payload.len()];
        client.read_exact(&mut got).await.unwrap();
        assert_eq!(got, payload);

        server.await.unwrap();
        let _ = tokio::fs::remove_file(path).await;
    }
}
