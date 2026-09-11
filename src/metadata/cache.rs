//! Caching strategies for metadata fetching.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::sync::Mutex;

use crate::error::AsyncTiffResult;
use crate::metadata::MetadataFetch;

/// Logic for managing a cache of sequential buffers
#[derive(Debug)]
struct SequentialBlockCache {
    /// Contiguous blocks from offset 0
    ///
    /// # Invariant
    /// - Buffers are contiguous from offset 0
    buffers: Vec<Bytes>,

    /// Total length cached (== sum of buffers lengths)
    len: u64,
}

impl SequentialBlockCache {
    /// Create a new, empty SequentialBlockCache
    fn new() -> Self {
        Self {
            buffers: vec![],
            len: 0,
        }
    }

    /// Check if the given range is fully contained within the cached buffers
    fn contains(&self, range: Range<u64>) -> bool {
        range.end <= self.len
    }

    /// Slice out the given range from the cached buffers
    fn slice(&self, range: Range<u64>) -> Bytes {
        // The size of the output buffer
        let out_len = (range.end - range.start) as usize;

        // The remaining range of bytes required. This range is updated as we traverse buffers, so
        // the indexes are relative to the current buffer.
        let mut remaining = range;
        let mut out_buffers: Vec<Bytes> = vec![];

        for buf in &self.buffers {
            let current_buf_len = buf.len() as u64;

            // this block falls entirely before the desired range start
            if remaining.start >= current_buf_len {
                remaining.start -= current_buf_len;
                remaining.end -= current_buf_len;
                continue;
            }

            // we slice bytes out of *this* block
            let start = remaining.start as usize;
            let length =
                (remaining.end - remaining.start).min(current_buf_len - remaining.start) as usize;
            let end = start + length;

            // nothing to take from this block
            if start == end {
                continue;
            }

            let chunk = buf.slice(start..end);
            out_buffers.push(chunk);

            // consumed some portion; update and potentially break
            remaining.start = 0;
            if remaining.end <= current_buf_len {
                break;
            }
            remaining.end -= current_buf_len;
        }

        if out_buffers.len() == 1 {
            out_buffers.into_iter().next().unwrap()
        } else {
            let mut out = BytesMut::with_capacity(out_len);
            for b in out_buffers {
                out.extend_from_slice(&b);
            }
            out.into()
        }
    }

    fn append_buffer(&mut self, buffer: Bytes) {
        self.len += buffer.len() as u64;
        self.buffers.push(buffer);
    }
}

/// Logic for managing one cached window at an arbitrary offset
///
/// The sequential cache above is contiguous from zero, which cannot describe a read far into the
/// file. This holds a single window instead, replaced whenever a request falls outside it.
#[derive(Debug, Default)]
struct WindowCache {
    /// Where `buffer` begins in the file
    start: u64,
    buffer: Bytes,
}

impl WindowCache {
    fn contains(&self, range: &Range<u64>) -> bool {
        !self.buffer.is_empty()
            && range.start >= self.start
            && range.end <= self.start + self.buffer.len() as u64
    }

    fn slice(&self, range: Range<u64>) -> Bytes {
        let from = (range.start - self.start) as usize;
        let to = (range.end - self.start) as usize;
        self.buffer.slice(from..to)
    }
}

/// A MetadataFetch implementation that caches fetched data in exponentially growing chunks,
/// sequentially from the beginning of the file.
#[derive(Debug)]
pub struct ReadaheadMetadataCache<F: MetadataFetch> {
    inner: F,
    cache: Arc<Mutex<SequentialBlockCache>>,
    /// For ranges the sequential cache cannot reach without fetching everything before them
    window: Arc<Mutex<WindowCache>>,
    initial: u64,
    multiplier: f64,
}

impl<F: MetadataFetch> ReadaheadMetadataCache<F> {
    /// Create a new ReadaheadMetadataCache wrapping the given MetadataFetch
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            cache: Arc::new(Mutex::new(SequentialBlockCache::new())),
            window: Arc::new(Mutex::new(WindowCache::default())),
            initial: 32 * 1024,
            multiplier: 2.0,
        }
    }

    /// Access the inner MetadataFetch
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Set the initial fetch size in bytes, otherwise defaults to 32 KiB
    pub fn with_initial_size(mut self, initial: u64) -> Self {
        self.initial = initial;
        self
    }

    /// Set the multiplier for subsequent fetch sizes, otherwise defaults to 2.0
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Fetch a range that lies far beyond the sequential cache, through the window.
    ///
    /// Reads here are as small as a single tag entry, so a fetch of exactly what is asked would
    /// send one request per entry — thousands, for an image with thousands of tiles. The window is
    /// filled with at least `initial` bytes so that the reads after a miss come from memory, for
    /// the same reason the sequential cache reads ahead at the front of the file.
    async fn fetch_distant(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        let mut window = self.window.lock().await;
        if window.contains(&range) {
            return Ok(window.slice(range));
        }

        let wanted = (range.end - range.start).max(self.initial);
        let bytes = self.inner.fetch(range.start..range.start + wanted).await?;

        // A window at the end of the file comes back short, which is expected: a store clamps the
        // range to the object. It is only a problem if it fails to cover what was asked for.
        if (bytes.len() as u64) < range.end - range.start {
            return self.inner.fetch(range).await;
        }

        *window = WindowCache {
            start: range.start,
            buffer: bytes,
        };
        Ok(window.slice(range))
    }

    fn next_fetch_size(&self, existing_len: u64) -> u64 {
        if existing_len == 0 {
            self.initial
        } else {
            (existing_len as f64 * self.multiplier).round() as u64
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl<F: MetadataFetch + Send + Sync> MetadataFetch for ReadaheadMetadataCache<F> {
    async fn fetch(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        let mut cache = self.cache.lock().await;

        // First check if we already have the range cached
        if cache.contains(range.start..range.end) {
            return Ok(cache.slice(range));
        }

        // Compute the correct fetch range
        let start_len = cache.len;
        let readahead = self.next_fetch_size(start_len);

        // A range far beyond what is cached means the metadata is not near the front of the file.
        // Writers that stream image data put their IFDs after it, so the first IFD of a 17 GB image
        // can sit in its last kilobytes. Growing the sequential cache to reach it would fetch
        // everything in between -- the whole file -- as one request, which fails or hangs rather
        // than being merely wasteful. Read such a range where it is, and leave the cache alone: it
        // is contiguous from zero by construction, and the bytes in between are never wanted.
        if range.start > start_len + readahead {
            drop(cache);
            return self.fetch_distant(range).await;
        }

        let needed = range.end.saturating_sub(start_len);
        let fetch_size = readahead.max(needed);
        let fetch_range = start_len..start_len + fetch_size;

        // Perform the fetch while holding mutex
        // (this is OK because the mutex is async)
        let bytes = self.inner.fetch(fetch_range).await?;

        // Now append safely
        cache.append_buffer(bytes);

        Ok(cache.slice(range))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct TestFetch {
        data: Bytes,
        /// The number of fetches that actually reach the raw Fetch implementation
        num_fetches: Arc<Mutex<u64>>,
    }

    impl TestFetch {
        fn new(data: Bytes) -> Self {
            Self {
                data,
                num_fetches: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    impl MetadataFetch for TestFetch {
        async fn fetch(&self, range: Range<u64>) -> crate::error::AsyncTiffResult<Bytes> {
            if range.start as usize >= self.data.len() {
                return Ok(Bytes::new());
            }

            let end = (range.end as usize).min(self.data.len());
            let slice = self.data.slice(range.start as _..end);
            let mut g = self.num_fetches.lock().await;
            *g += 1;
            Ok(slice)
        }
    }

    #[tokio::test]
    async fn test_metadata_far_from_the_start_is_not_reached_by_fetching_everything_before_it() {
        // A file whose metadata sits at the end, as TIFF writers that stream image data produce.
        let mut data = vec![b'.'; 4096];
        data.extend_from_slice(b"metadata-at-the-end");
        let data = Bytes::from(data);
        let fetch = TestFetch::new(data.clone());
        let counter = fetch.num_fetches.clone();
        // A 64-byte window is large enough for the reads below and far smaller than the
        // 32 KiB default, so the test does not depend on that default.
        let cache = ReadaheadMetadataCache::new(fetch).with_initial_size(64);

        // A header read at the front, as a reader does first.
        assert_eq!(cache.fetch(0..4).await.unwrap(), data.slice(0..4));

        // Then a read at the end. The sequential cache would have to fetch the 4 KiB in between.
        let tail = cache.fetch(4096..4103).await.unwrap();
        assert_eq!(tail, data.slice(4096..4103));

        // And the reads that follow it come from the window rather than the wire.
        let fetches_after_the_jump = *counter.lock().await;
        assert_eq!(
            cache.fetch(4103..4108).await.unwrap(),
            data.slice(4103..4108)
        );
        assert_eq!(
            *counter.lock().await,
            fetches_after_the_jump,
            "a read inside the window should not fetch again"
        );
    }

    #[tokio::test]
    async fn test_readahead_cache() {
        let data = Bytes::from_static(b"abcdefghijklmnopqrstuvwxyz");
        let fetch = TestFetch::new(data.clone());
        let cache = ReadaheadMetadataCache::new(fetch)
            .with_initial_size(2)
            .with_multiplier(3.0);

        // Make initial request
        let result = cache.fetch(0..2).await.unwrap();
        assert_eq!(result.as_ref(), b"ab");
        assert_eq!(*cache.inner.num_fetches.lock().await, 1);

        // Making a request within the cached range should not trigger a new fetch
        let result = cache.fetch(1..2).await.unwrap();
        assert_eq!(result.as_ref(), b"b");
        assert_eq!(*cache.inner.num_fetches.lock().await, 1);

        // Making a request that exceeds the cached range should trigger a new fetch
        let result = cache.fetch(2..5).await.unwrap();
        assert_eq!(result.as_ref(), b"cde");
        assert_eq!(*cache.inner.num_fetches.lock().await, 2);

        // Multiplier should be accurate: initial was 2, next was 6 (2*3), so total cached is now 8
        let result = cache.fetch(5..8).await.unwrap();
        assert_eq!(result.as_ref(), b"fgh");
        assert_eq!(*cache.inner.num_fetches.lock().await, 2);

        // Should work even for fetch range larger than underlying buffer
        let result = cache.fetch(8..20).await.unwrap();
        assert_eq!(result.as_ref(), b"ijklmnopqrst");
        assert_eq!(*cache.inner.num_fetches.lock().await, 3);
    }

    #[test]
    fn test_sequential_block_cache_empty_buffers() {
        let mut cache = SequentialBlockCache::new();
        cache.append_buffer(Bytes::from_static(b"012"));
        cache.append_buffer(Bytes::from_static(b""));
        cache.append_buffer(Bytes::from_static(b"34"));
        cache.append_buffer(Bytes::from_static(b""));
        cache.append_buffer(Bytes::from_static(b"5"));
        cache.append_buffer(Bytes::from_static(b""));
        cache.append_buffer(Bytes::from_static(b"67"));

        // Range, does it exist, expected slice
        let test_cases = [
            (0..3, true, Bytes::from_static(b"012")),
            (4..7, true, Bytes::from_static(b"456")),
            (0..8, true, Bytes::from_static(b"01234567")),
            (6..6, true, Bytes::from_static(b"")),
            (6..9, false, Bytes::from_static(b"")),
            (9..9, false, Bytes::from_static(b"")),
            (8..10, false, Bytes::from_static(b"")),
        ];

        for (range, exists, expected) in test_cases {
            assert_eq!(cache.contains(range.clone()), exists);
            if exists {
                assert_eq!(cache.slice(range.clone()), expected);
            }
        }
    }
}
