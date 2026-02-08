use bytes::Bytes;
use bytes::BytesMut;
use memchr::memchr;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;

#[derive(Debug)]
pub struct LineReader<R> {
    inner: R,
    buf: BytesMut,
    max_line_len: usize,
}

impl<R> LineReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: BytesMut::with_capacity(8 * 1024),
            max_line_len: 8 * 1024,
        }
    }

    pub fn with_capacity(inner: R, cap: usize) -> Self {
        Self {
            inner,
            buf: BytesMut::with_capacity(cap),
            max_line_len: cap.max(1),
        }
    }

    pub fn max_line_len(mut self, max: usize) -> Self {
        self.max_line_len = max.max(1);
        self
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    /// Read one line, stripping trailing `\n` and optional `\r`.
    ///
    /// Returns:
    /// - `Ok(Some(bytes))` for a line (may be empty),
    /// - `Ok(None)` on clean EOF with no buffered data.
    pub async fn read_line(&mut self) -> std::io::Result<Option<Bytes>> {
        loop {
            if let Some(i) = memchr(b'\n', &self.buf) {
                let raw = self.buf.split_to(i + 1).freeze();
                return Ok(Some(trim_crlf(raw)));
            }

            if self.buf.len() > self.max_line_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "line too long",
                ));
            }

            let n = self.inner.read_buf(&mut self.buf).await?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof while reading line",
                ));
            }
        }
    }
}

fn trim_crlf(mut b: Bytes) -> Bytes {
    let mut end = b.len();
    if end > 0 && b[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && b[end - 1] == b'\r' {
        end -= 1;
    }
    b.truncate(end);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn reads_crlf_and_lf() {
        let (a, b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let mut b = b;
            b.write_all(b"hello\r\nworld\n").await.unwrap();
        });

        let mut lr = LineReader::new(a);
        let l1 = lr.read_line().await.unwrap().unwrap();
        let l2 = lr.read_line().await.unwrap().unwrap();
        assert_eq!(&l1[..], b"hello");
        assert_eq!(&l2[..], b"world");
    }
}
