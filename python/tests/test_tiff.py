from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np
import pytest
from rasterio.errors import NotGeoreferencedWarning
from rasterio.plot import reshape_as_image
from rasterio.windows import Window

if TYPE_CHECKING:
    from .conftest import LoadRasterio, LoadTIFF


@pytest.mark.asyncio
@pytest.mark.parametrize(("variant", "file_name"), [("eox", "eox_cloudless")])
async def test_read_band_interleaved_tiff_window(
    load_tiff: LoadTIFF,
    load_rasterio: LoadRasterio,
    variant: str,
    file_name: str,
) -> None:
    tiff = await load_tiff(file_name, variant=variant)

    tile = await tiff.ifds[0].fetch_tile(0, 0)
    array = await tile.decode()
    data = np.array(array)

    window = Window(0, 0, tiff.ifds[0].tile_width, tiff.ifds[0].tile_height)
    with load_rasterio(file_name, variant=variant) as rasterio_ds:
        rasterio_data = rasterio_ds.read(window=window)

    np.testing.assert_array_equal(data, rasterio_data)


@pytest.mark.asyncio
@pytest.mark.parametrize(("variant", "file_name"), [("eox", "eox_cloudless")])
async def test_header_byte_size(
    load_tiff: LoadTIFF,
    variant: str,
    file_name: str,
) -> None:
    tiff = await load_tiff(file_name, variant=variant)

    header = tiff.header_byte_size
    assert isinstance(header, int)
    assert header > 0

    expected = min(
        offset
        for ifd in tiff.ifds
        for offsets in (ifd.tile_offsets, ifd.strip_offsets)
        if offsets is not None
        for offset in offsets
        if offset != 0
    )
    assert header == expected


@pytest.mark.asyncio
async def test_first_ifd_at_the_end_of_the_file(
    load_tiff: LoadTIFF,
    load_rasterio: LoadRasterio,
) -> None:
    """A streaming writer's directory follows the image data instead of the header."""
    tiff = await load_tiff("trailing_directory", variant="tifffile")
    ifd = tiff.ifds[0]
    assert (ifd.image_height, ifd.image_width) == (256, 256)

    tile = await ifd.fetch_tile(0, 0)
    array = await tile.decode()
    data = np.array(array)

    window = Window(0, 0, ifd.tile_width, ifd.tile_height)
    # The fixture carries no georeferencing, so rasterio warns.
    with (
        pytest.warns(NotGeoreferencedWarning),
        load_rasterio("trailing_directory", variant="tifffile") as rasterio_ds,
    ):
        np.testing.assert_array_equal(
            data, reshape_as_image(rasterio_ds.read(window=window))
        )
