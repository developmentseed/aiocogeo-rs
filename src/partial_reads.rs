struct TileMetadata {
    /// top left corner of the partial read
    tlx: f64,
    tly: f64,
    /// width and height of the partial read (# of pixels)
    width: usize,
    height: usize,
    /// width and height of each block (# of pixels)
    tile_width: usize,
    tile_height: usize,
    /// range of internal x/y blocks which intersect the partial read
    xmin: usize,
    ymin: usize,
    xmax: usize,
    ymax: usize,
    /// expected number of bands
    bands: usize,
    /// numpy data type
    // dtype: np.dtype,
    /// overview level (where 0 is source)
    ovr_level: usize,
}
