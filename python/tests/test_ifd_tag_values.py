"""Tags whose values are IFD offsets.

The tag that matters in practice is 330, SubIFDs, which microscopy writers use for pyramid levels:
typed IFD in a classic TIFF and IFD8 in a BigTIFF. The fixtures come from geotiff-test-data's
tifffile_generated directory, one per type; each fixture's _info.md documents its layout.
"""

import pytest

SUBIFDS_TAG = 330

FIXTURES = pytest.mark.parametrize(
    "name",
    ["subifd_pyramid_classic", "subifd_pyramid_bigtiff"],
    ids=["classic", "bigtiff"],
)


def subifd_offsets(tiff):
    offsets = tiff.ifds[0].other_tags[SUBIFDS_TAG]
    return [offsets] if isinstance(offsets, int) else list(offsets)


@FIXTURES
async def test_subifds_tag_reads_as_offsets(load_tiff, fixtures_dir, name):
    """The tag arrives as integers, and the file opens, for both variants."""
    tiff = await load_tiff(name, variant="tifffile")
    offsets = subifd_offsets(tiff)

    assert offsets, "the tag carried no offsets"
    assert all(isinstance(offset, int) for offset in offsets)
    # An offset points inside the file, past the header.
    size = (
        (fixtures_dir / f"geotiff-test-data/tifffile_generated/fixtures/{name}.tif")
        .stat()
        .st_size
    )
    assert all(0 < offset < size for offset in offsets)


@FIXTURES
async def test_subifds_can_be_read_at_their_offsets(load_tiff, name):
    """And the levels they point at are reachable, which is the reason to expose the tag.

    `ifds` follows the top-level chain, which SubIFDs are not part of, so a walk of the chain
    alone finds one resolution level out of the total in the file.
    """
    tiff = await load_tiff(name, variant="tifffile")
    assert len(tiff.ifds) == 1, "the reduced levels are not in the top-level chain"

    for offset, size in zip(subifd_offsets(tiff), (128, 64), strict=True):
        level = await tiff.ifd_at(offset)
        assert (level.image_height, level.image_width) == (size, size)


@FIXTURES
async def test_the_image_beside_the_tag_still_reads(load_tiff, name):
    """And the IFD that carries the tag is otherwise unaffected: its tiles are at the offsets it stores."""
    tiff = await load_tiff(name, variant="tifffile")
    ifd = tiff.ifds[0]
    assert (ifd.image_height, ifd.image_width) == (256, 256)
    assert ifd.tile_count == (4, 4)
    assert len(ifd.tile_offsets) == 16
