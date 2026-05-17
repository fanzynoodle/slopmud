use std::io::{Error, ErrorKind, IoSlice};

use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};

const MAX_WRITEV_SLICES: usize = 64;

pub async fn write_all_vectored<W>(writer: &mut W, chunks: &[&[u8]]) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut index = 0usize;
    let mut offset = 0usize;

    while index < chunks.len() {
        while index < chunks.len() && offset >= chunks[index].len() {
            index += 1;
            offset = 0;
        }
        if index >= chunks.len() {
            break;
        }

        let mut slices =
            Vec::with_capacity(MAX_WRITEV_SLICES.min(chunks.len().saturating_sub(index)));
        for (pos, chunk) in chunks[index..].iter().enumerate() {
            if slices.len() >= MAX_WRITEV_SLICES {
                break;
            }
            let start = if pos == 0 { offset } else { 0 };
            if start < chunk.len() {
                slices.push(IoSlice::new(&chunk[start..]));
            }
        }

        if slices.is_empty() {
            break;
        }

        let written = writer.write_vectored(&slices).await?;
        if written == 0 {
            return Err(Error::new(
                ErrorKind::WriteZero,
                "failed to write vectored buffers",
            ));
        }

        advance(chunks, &mut index, &mut offset, written);
    }

    Ok(())
}

pub async fn write_all_bytes_vectored<W>(writer: &mut W, chunks: &[Bytes]) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let refs = chunks.iter().map(|b| b.as_ref()).collect::<Vec<_>>();
    write_all_vectored(writer, &refs).await
}

fn advance(chunks: &[&[u8]], index: &mut usize, offset: &mut usize, mut written: usize) {
    while written > 0 && *index < chunks.len() {
        let available = chunks[*index].len().saturating_sub(*offset);
        if written < available {
            *offset += written;
            return;
        }

        written = written.saturating_sub(available);
        *index += 1;
        *offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn writes_many_byte_chunks_in_order() {
        let (mut rd, mut wr) = tokio::io::duplex(128);
        let chunks = (0..96)
            .map(|i| Bytes::from(format!("{i:02}")))
            .collect::<Vec<_>>();

        let writer = tokio::spawn(async move {
            write_all_bytes_vectored(&mut wr, &chunks).await.unwrap();
        });

        let mut out = Vec::new();
        rd.read_to_end(&mut out).await.unwrap();
        writer.await.unwrap();

        let expected = (0..96).map(|i| format!("{i:02}")).collect::<String>();
        assert_eq!(out, expected.as_bytes());
    }
}
