from collections.abc import Buffer

from ._array import Array
from ._decoder import DecoderRegistry
from ._thread_pool import ThreadPool
from .enums import Compression

class Tile:
    """A representation of a TIFF image tile."""
    @property
    def x(self) -> int:
        """The column index this tile represents."""
    @property
    def y(self) -> int:
        """The row index this tile represents."""
    @property
    def compressed_bytes(self) -> Buffer | None | list[Buffer | None]:
        """The compressed bytes underlying this tile.

        This will be a single buffer for pixel-interleaved (chunky) data, or a list of
        buffers for band-interleaved (planar) data. A `None` entry means that tile or band
        was never written by the encoder (sparse); see `is_sparse`.
        """
    @property
    def compression_method(self) -> Compression | int:
        """The compression method used by this tile."""
    @property
    def is_sparse(self) -> bool:
        """Whether this tile is entirely sparse (never written by the encoder).

        `False` for a planar tile with only some bands sparse; inspect
        `compressed_bytes` for per-band detail in that case. `decode` still succeeds for a
        sparse tile, filling it with the IFD's nodata value (or `0`).
        """
    def decode_sync(
        self,
        *,
        decoder_registry: DecoderRegistry | None = None,
    ) -> Array:
        """Decode this tile's data.

        **Note**: This is a blocking function and will perform the tile decompression on
        the current thread. Prefer using the asynchronous `decode` method, which will
        offload decompression to a thread pool.

        Keyword Args:
            decoder_registry: the decoders to use for decompression. Defaults to None, in which case a default decoder registry is used.

        Returns:
            Decoded tile data as an Array instance.
        """

    async def decode(
        self,
        *,
        decoder_registry: DecoderRegistry | None = None,
        pool: ThreadPool | None = None,
    ) -> Array:
        """Decode this tile's data.

        This is an asynchronous function that will offload the tile decompression to a
        thread pool.

        Keyword Args:
            decoder_registry: the decoders to use for decompression. Defaults to None, in which case a default decoder registry is used.
            pool: the thread pool on which to run decompression. Defaults to None, in
                which case, a default thread pool is used.

        Returns:
            Decoded tile data as an Array instance.
        """
