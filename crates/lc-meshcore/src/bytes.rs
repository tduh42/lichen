//! Panic-free byte reading and writing.

use crate::EncodeError;

#[derive(Debug)]
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.buf.len() {
            return None;
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Some(head)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<&'a [u8; N]> {
        self.bytes(N).and_then(|b| b.try_into().ok())
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|b| b[0])
    }

    pub(crate) fn i8(&mut self) -> Option<i8> {
        self.u8().map(|b| b as i8)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array::<2>().map(|b| u16::from_le_bytes(*b))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array::<4>().map(|b| u32::from_le_bytes(*b))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        self.array::<4>().map(|b| i32::from_le_bytes(*b))
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        core::mem::take(&mut self.buf)
    }

    pub(crate) const fn remaining(&self) -> usize {
        self.buf.len()
    }
}

#[derive(Debug)]
pub(crate) struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn put(&mut self, data: &[u8]) -> Result<(), EncodeError> {
        let end = self
            .pos
            .checked_add(data.len())
            .ok_or(EncodeError::Overflow)?;
        let dst = self
            .buf
            .get_mut(self.pos..end)
            .ok_or(EncodeError::Overflow)?;
        dst.copy_from_slice(data);
        self.pos = end;
        Ok(())
    }

    pub(crate) fn u8(&mut self, v: u8) -> Result<(), EncodeError> {
        self.put(&[v])
    }

    pub(crate) fn u16(&mut self, v: u16) -> Result<(), EncodeError> {
        self.put(&v.to_le_bytes())
    }

    pub(crate) fn u32(&mut self, v: u32) -> Result<(), EncodeError> {
        self.put(&v.to_le_bytes())
    }

    pub(crate) const fn len(&self) -> usize {
        self.pos
    }
}
