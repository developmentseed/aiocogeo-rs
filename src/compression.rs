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
