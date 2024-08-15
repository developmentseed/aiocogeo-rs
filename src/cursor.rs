use std::sync::Arc;

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

    pub(crate) fn seek(&mut self, offset: usize) {
        self.offset = offset;
    }

    fn tell(&self) -> usize {
        self.offset
    }
}
