//! Bytes that held a secret: a password or PIN on its way to whatever
//! checks it.

use std::fmt;

use zeroize::Zeroize;

/// A byte buffer wiped when dropped, its spare capacity too, with writes
/// the compiler cannot skip. It never prints its bytes and cannot be
/// cloned. Its capacity is fixed at creation and never grows: growing
/// would move the bytes and free the old copy unwiped.
///
/// TODO: keep it in unswappable memory (`mlock`, and `MADV_DONTDUMP` for
/// core dumps), so it can never reach a swap device or a dump. That needs
/// an allocation of its own pages rather than the heap's.
#[derive(Default)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// An empty secret that can hold `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Self {
        Secret(Vec::with_capacity(capacity))
    }

    /// Add `byte`. A full secret refuses it rather than grow.
    pub fn push(&mut self, byte: u8) {
        if self.0.len() < self.0.capacity() {
            self.0.push(byte);
        }
    }

    /// Remove the last byte, wiping it.
    pub fn pop(&mut self) {
        if let Some(last) = self.0.last_mut() {
            last.zeroize();
            self.0.pop();
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

/// Takes the bytes over without copying them.
impl From<Vec<u8>> for Secret {
    fn from(bytes: Vec<u8>) -> Self {
        Secret(bytes)
    }
}

/// Takes the text over without copying it.
impl From<String> for Secret {
    fn from(text: String) -> Self {
        Secret(text.into_bytes())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_up_to_its_capacity_and_pops() {
        let mut s = Secret::with_capacity(2);
        let capacity = s.0.capacity();
        for _ in 0..=capacity {
            s.push(b'1');
        }
        assert_eq!(s.len(), capacity, "never grows");
        s.0.truncate(2);
        assert_eq!(s.as_bytes(), b"11");
        s.pop();
        assert_eq!(s.as_bytes(), b"1");
    }

    #[test]
    fn never_prints_its_bytes() {
        assert_eq!(
            format!("{:?}", Secret::from(b"1234".to_vec())),
            "Secret(..)"
        );
    }
}
