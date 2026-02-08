use bytes::Buf;
use bytes::Bytes;
use bytes::BytesMut;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

#[derive(Debug)]
pub struct FrameReader<R> {
    inner: R,
    buf: BytesMut,
    max_frame_len: usize,
}

impl<R> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: BytesMut::with_capacity(8 * 1024),
            max_frame_len: 8 * 1024 * 1024,
        }
    }

    pub fn max_frame_len(mut self, max: usize) -> Self {
        self.max_frame_len = max.max(1);
        self
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// Read one frame with a `u32` big-endian length prefix.
    ///
    /// Returns:
    /// - `Ok(Some(payload))` for a frame payload,
    /// - `Ok(None)` on clean EOF with no buffered data.
    pub async fn read_frame(&mut self) -> std::io::Result<Option<Bytes>> {
        loop {
            if self.buf.len() >= 4 {
                let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]])
                    as usize;
                if len > self.max_frame_len {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "frame too large",
                    ));
                }

                if self.buf.len() >= 4 + len {
                    self.buf.advance(4);
                    let payload = self.buf.split_to(len).freeze();
                    return Ok(Some(payload));
                }
            }

            let n = self.inner.read_buf(&mut self.buf).await?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof while reading frame",
                ));
            }
        }
    }
}

#[derive(Debug)]
pub struct FrameWriter<W> {
    inner: W,
}

impl<W> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub async fn write_frame(&mut self, payload: &[u8]) -> std::io::Result<()> {
        self.write_frame_parts(&[payload]).await
    }

    /// Write a frame without concatenating payload parts.
    ///
    /// This avoids an extra copy when the payload already lives in separate buffers
    /// (e.g., `[type+session_id]` header plus a `Bytes` body).
    pub async fn write_frame_parts(&mut self, parts: &[&[u8]]) -> std::io::Result<()> {
        let len: usize = parts.iter().map(|p| p.len()).sum();
        let len_u32: u32 = len
            .try_into()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too big"))?;

        self.inner.write_all(&len_u32.to_be_bytes()).await?;
        for p in parts {
            if !p.is_empty() {
                self.inner.write_all(p).await?;
            }
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_frame() {
        let (a, b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let mut fw = FrameWriter::new(b);
            fw.write_frame(b"abc").await.unwrap();
            fw.flush().await.unwrap();
        });

        let mut fr = FrameReader::new(a);
        let f = fr.read_frame().await.unwrap().unwrap();
        assert_eq!(&f[..], b"abc");
    }

    #[tokio::test]
    async fn writes_parts_without_concat() {
        let (a, mut b) = tokio::io::duplex(64);
        let mut fw = FrameWriter::new(a);
        fw.write_frame_parts(&[b"he", b"llo"]).await.unwrap();
        fw.flush().await.unwrap();

        let mut fr = FrameReader::new(&mut b);
        let f = fr.read_frame().await.unwrap().unwrap();
        assert_eq!(&f[..], b"hello");
    }
}
