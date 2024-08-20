use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Clone, Copy, Debug, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
enum Compression {
    Uncompressed = 1,
    Lzw = 5,
    // TODO: can jpeg be 6 or 7?
    Jpeg = 6,
    // Jpeg = 7,
    Deflate = 8,
    Packbits = 32773,
    Webp = 50001,
}

trait Decompressor {
    // TODO: should this return an ndarray?
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8>;
}

pub(crate) struct JPEGDecompressor {}

impl Decompressor for JPEGDecompressor {
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8> {
        todo!()
    }
}

pub(crate) struct LZWDecompressor {}

impl Decompressor for LZWDecompressor {
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8> {
        todo!()
    }
}

pub(crate) struct WebPDecompressor {}

impl Decompressor for WebPDecompressor {
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8> {
        todo!()
    }
}

pub(crate) struct DeflateDecompressor {}

impl Decompressor for DeflateDecompressor {
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8> {
        todo!()
    }
}

pub(crate) struct PackbitsDecompressor {}

impl Decompressor for PackbitsDecompressor {
    fn decompress(&self, tile: Vec<u8>) -> Vec<u8> {
        todo!()
    }
}
