use std::io::{Read, Result as IoResult};

/// Abstract content access — not tied to `std::fs::File`.
pub trait ContentReader: Send {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize>;
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> IoResult<usize> {
        let mut total = 0;
        let mut tmp = [0u8; 8192];
        loop {
            let n = self.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            total += n;
        }
        Ok(total)
    }
}

pub struct BytesContentReader {
    data: Vec<u8>,
    pos: usize,
}

impl BytesContentReader {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        Self::new(data.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn as_str_lossy(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

impl ContentReader for BytesContentReader {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = (self.data.len() - self.pos).min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

pub struct LimitedReader<R: Read> {
    inner: R,
    remaining: u64,
}

impl<R: Read> LimitedReader<R> {
    pub fn new(inner: R, max_bytes: u64) -> Self {
        Self {
            inner,
            remaining: max_bytes,
        }
    }
}

impl<R: Read + Send> ContentReader for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = (self.remaining as usize).min(buf.len());
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining = self.remaining.saturating_sub(n as u64);
        Ok(n)
    }
}
