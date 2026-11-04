#[derive(Clone, Copy, Default, Debug)]
pub struct EncoderOptions {
    pub try_vaapi: bool,
    pub try_nvenc: bool,
    pub try_videotoolbox: bool,
    pub try_mediafoundation: bool,
}

pub enum PixelProvider<'a> {
    RGB(usize, usize, &'a [u8]),
    RGB0(usize, usize, &'a [u8]),
    BGR0(usize, usize, &'a [u8]),
    BGR0S(usize, usize, usize, &'a [u8]),
}

impl<'a> PixelProvider<'a> {
    pub fn size(&self) -> (usize, usize) {
        match self {
            PixelProvider::RGB(w, h, _) => (*w, *h),
            PixelProvider::RGB0(w, h, _) => (*w, *h),
            PixelProvider::BGR0(w, h, _) => (*w, *h),
            PixelProvider::BGR0S(w, h, _, _) => (*w, *h),
        }
    }
}

pub struct VideoEncoder;

impl VideoEncoder {
    pub fn new(
        _width_in: usize,
        _height_in: usize,
        _width_out: usize,
        _height_out: usize,
        _write_data: impl FnMut(&[u8]) + 'static,
        _options: EncoderOptions,
    ) -> Result<Box<Self>, crate::cerror::CError> {
        Ok(Box::new(Self))
    }

    pub fn check_size(&self, _w_in: usize, _h_in: usize, _w_out: usize, _h_out: usize) -> bool {
        true
    }

    pub fn encode(&mut self, _pixel_data: PixelProvider) {}
}
