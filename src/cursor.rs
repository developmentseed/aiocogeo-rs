use std::io::Cursor;
use std::sync::Arc;

use byteorder::{ByteOrder, ReadBytesExt};
use bytes::Bytes;
use object_store::path::Path;
use object_store::ObjectStore;

/// A wrapper around an [ObjectStore] that provides a seek-oriented interface
// TODO: in the future add buffering to this
pub(crate) struct ObjectStoreCursor {
    store: Arc<dyn ObjectStore>,
    path: Path,
    offset: usize,
}

/// Macro to generate functions to read scalar values from the cursor
macro_rules! impl_read_byteorder {
    ($method_name:ident, $typ:ty) => {
        pub(crate) async fn $method_name<T: ByteOrder>(&mut self) -> $typ {
            let buf = self.read(<$typ>::BITS as usize / 8).await;
            Cursor::new(buf).$method_name::<T>().unwrap()
        }
    };
}

impl ObjectStoreCursor {
    pub(crate) fn new(store: Arc<dyn ObjectStore>, path: Path) -> Self {
        Self {
            store,
            path,
            offset: 0,
        }
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

    pub(crate) async fn read_f32<T: ByteOrder>(&mut self) -> f32 {
        let buf = self.read(4).await;
        Cursor::new(buf).read_f32::<T>().unwrap()
    }

    pub(crate) async fn read_f64<T: ByteOrder>(&mut self) -> f64 {
        let buf = self.read(8).await;
        Cursor::new(buf).read_f64::<T>().unwrap()
    }

    pub(crate) fn seek(&mut self, offset: usize) {
        self.offset = offset;
    }

    fn tell(&self) -> usize {
        self.offset
    }
}
