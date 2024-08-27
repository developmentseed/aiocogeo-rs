use std::io::Cursor;
use std::ops::Range;
use std::sync::Arc;

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use bytes::Bytes;
use object_store::path::Path;
use object_store::ObjectStore;

#[derive(Debug, Clone, Copy, Default)]
pub enum Endianness {
    #[default]
    LittleEndian,
    BigEndian,
}

/// A wrapper around an [ObjectStore] that provides a seek-oriented interface
// TODO: in the future add buffering to this
pub(crate) struct ObjectStoreCursor {
    store: Arc<dyn ObjectStore>,
    path: Path,
    offset: usize,
    endianness: Endianness,
}

/// Macro to generate functions to read scalar values from the cursor
macro_rules! impl_read_byteorder {
    ($method_name:ident, $typ:ty) => {
        pub(crate) async fn $method_name(&mut self) -> $typ {
            let mut buf = Cursor::new(self.read(<$typ>::BITS as usize / 8).await);
            match self.endianness {
                Endianness::LittleEndian => buf.$method_name::<LittleEndian>().unwrap(),
                Endianness::BigEndian => buf.$method_name::<BigEndian>().unwrap(),
            }
        }
    };
}

impl ObjectStoreCursor {
    pub(crate) fn new(store: Arc<dyn ObjectStore>, path: Path) -> Self {
        Self {
            store,
            path,
            offset: 0,
            endianness: Default::default(),
        }
    }

    pub(crate) fn set_endianness(&mut self, endianness: Endianness) {
        self.endianness = endianness;
    }

    pub(crate) fn into_inner(self) -> (Arc<dyn ObjectStore>, Path) {
        (self.store, self.path)
    }

    pub(crate) async fn read(&mut self, length: usize) -> Bytes {
        let range = self.offset..self.offset + length;
        self.offset += length;
        self.store.get_range(&self.path, range).await.unwrap()
    }

    /// Read a u8 from the cursor
    pub(crate) async fn read_u8(&mut self) -> u8 {
        let buf = self.read(u8::BITS as usize / 8).await;
        Cursor::new(buf).read_u8().unwrap()
    }

    /// Read a i8 from the cursor
    pub(crate) async fn read_i8(&mut self) -> i8 {
        let buf = self.read(1).await;
        Cursor::new(buf).read_i8().unwrap()
    }

    impl_read_byteorder!(read_u16, u16);
    impl_read_byteorder!(read_u32, u32);
    impl_read_byteorder!(read_u64, u64);
    impl_read_byteorder!(read_i16, i16);
    impl_read_byteorder!(read_i32, i32);
    impl_read_byteorder!(read_i64, i64);

    pub(crate) async fn read_f32(&mut self) -> f32 {
        let mut buf = Cursor::new(self.read(4).await);
        match self.endianness {
            Endianness::LittleEndian => buf.read_f32::<LittleEndian>().unwrap(),
            Endianness::BigEndian => buf.read_f32::<BigEndian>().unwrap(),
        }
    }

    pub(crate) async fn read_f64(&mut self) -> f64 {
        let mut buf = Cursor::new(self.read(8).await);
        match self.endianness {
            Endianness::LittleEndian => buf.read_f64::<LittleEndian>().unwrap(),
            Endianness::BigEndian => buf.read_f64::<BigEndian>().unwrap(),
        }
    }

    pub(crate) fn store(&self) -> &Arc<dyn ObjectStore> {
        &self.store
    }

    pub(crate) async fn get_range(
        &self,
        range: Range<usize>,
    ) -> Result<Bytes, object_store::Error> {
        Ok(self.store.get_range(&self.path, range).await?)
    }

    /// Advance cursor position by a set amount
    pub(crate) fn advance(&mut self, amount: usize) {
        self.offset += amount;
    }

    pub(crate) fn seek(&mut self, offset: usize) {
        self.offset = offset;
    }

    pub(crate) fn position(&self) -> usize {
        self.offset
    }
}
