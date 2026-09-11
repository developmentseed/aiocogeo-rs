use std::sync::Arc;

use async_tiff::metadata::cache::ReadaheadMetadataCache;
use async_tiff::metadata::TiffMetadataReader;
use async_tiff::reader::{AsyncFileReader, Endianness};
use async_tiff::ImageFileDirectory;
use pyo3::exceptions::{PyIndexError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyType;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::enums::PyEndianness;
use crate::error::PyAsyncTiffResult;
use crate::reader::StoreInput;
use crate::tile::PyTile;
use crate::PyImageFileDirectory;

#[pyclass(name = "TIFF", frozen, subclass)]
pub(crate) struct PyTIFF {
    endianness: Endianness,
    ifds: Vec<Arc<ImageFileDirectory>>,
    reader: Arc<dyn AsyncFileReader>,
}

async fn open(
    reader: Arc<dyn AsyncFileReader>,
    prefetch: u64,
    multiplier: f64,
) -> PyAsyncTiffResult<PyTIFF> {
    let metadata_fetch = ReadaheadMetadataCache::new(reader.clone())
        .with_initial_size(prefetch)
        .with_multiplier(multiplier);
    let mut metadata_reader = TiffMetadataReader::try_open(&metadata_fetch).await?;
    let ifds = metadata_reader.read_all_ifds(&metadata_fetch).await?;
    Ok(PyTIFF {
        endianness: metadata_reader.endianness(),
        ifds: ifds.into_iter().map(Arc::new).collect(),
        reader,
    })
}

async fn read_ifd_at(
    reader: Arc<dyn AsyncFileReader>,
    offset: u64,
    prefetch: u64,
) -> PyAsyncTiffResult<PyImageFileDirectory> {
    let metadata_fetch = ReadaheadMetadataCache::new(reader.clone()).with_initial_size(prefetch);
    // Re-reads the 16-byte header to learn the byte order and whether the file is a BigTIFF.
    // Those two facts decide the structure of the directory at `offset`.
    let metadata_reader = TiffMetadataReader::try_open(&metadata_fetch).await?;
    let ifd = metadata_reader.read_ifd_at(&metadata_fetch, offset).await?;
    Ok(PyImageFileDirectory::new(Arc::new(ifd), reader))
}

#[pymethods]
impl PyTIFF {
    #[classmethod]
    #[pyo3(signature = (path, *, store, prefetch=32768, multiplier=2.0))]
    fn open<'py>(
        _cls: &Bound<'py, PyType>,
        py: Python<'py>,
        path: String,
        store: StoreInput,
        prefetch: u64,
        multiplier: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = store.into_async_file_reader(path);

        let cog_reader =
            future_into_py(
                py,
                async move { Ok(open(reader, prefetch, multiplier).await?) },
            )?;
        Ok(cog_reader)
    }

    #[getter]
    fn endianness(&self) -> PyEndianness {
        self.endianness.into()
    }

    #[getter]
    fn header_byte_size(&self) -> u64 {
        self.ifds
            .iter()
            .flat_map(|ifd| {
                ifd.tile_offsets()
                    .into_iter()
                    .chain(ifd.strip_offsets())
                    .flatten()
                    .copied()
                    .filter(|&o| o != 0)
            })
            .min()
            .expect("TIFF spec requires every IFD to have StripOffsets or TileOffsets")
    }

    fn ifd(&self, index: usize) -> PyResult<PyImageFileDirectory> {
        let ifd = self
            .ifds
            .get(index)
            .ok_or_else(|| PyIndexError::new_err(format!("No IFD found for index={index}")))?
            .clone();
        Ok(PyImageFileDirectory::new(ifd, self.reader.clone()))
    }

    /// Read an IFD at a byte offset, for directories outside the top-level chain.
    ///
    /// `ifds` follows the chain, which does not include SubIFDs (tag 330); those offsets are values
    /// of that tag, and pyramid levels are commonly written there.
    #[pyo3(signature = (offset, *, prefetch=32768))]
    fn ifd_at<'py>(
        &'py self,
        py: Python<'py>,
        offset: u64,
        prefetch: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.reader.clone();
        future_into_py(py, async move {
            Ok(read_ifd_at(reader, offset, prefetch).await?)
        })
    }

    #[getter]
    fn ifds(&self) -> Vec<PyImageFileDirectory> {
        self.ifds
            .iter()
            .map(|ifd| PyImageFileDirectory::new(ifd.clone(), self.reader.clone()))
            .collect()
    }

    fn fetch_tile<'py>(
        &'py self,
        py: Python<'py>,
        x: usize,
        y: usize,
        z: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.reader.clone();
        let ifd = self
            .ifds
            .get(z)
            .ok_or_else(|| PyIndexError::new_err(format!("No IFD found for z={z}")))?
            .clone();
        future_into_py(py, async move {
            let tile = ifd
                .fetch_tile(x, y, reader.as_ref())
                .await
                .map_err(|err| PyTypeError::new_err(err.to_string()))?;

            Ok(PyTile::new(tile))
        })
    }

    fn fetch_tiles<'py>(
        &'py self,
        py: Python<'py>,
        xy: Vec<(usize, usize)>,
        z: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.reader.clone();
        let ifd = self
            .ifds
            .get(z)
            .ok_or_else(|| PyIndexError::new_err(format!("No IFD found for z={z}")))?
            .clone();
        future_into_py(py, async move {
            let tiles = ifd
                .fetch_tiles(&xy, reader.as_ref())
                .await
                .map_err(|err| PyTypeError::new_err(err.to_string()))?;
            let py_tiles = tiles.into_iter().map(PyTile::new).collect::<Vec<_>>();
            Ok(py_tiles)
        })
    }
}
