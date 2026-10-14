//! Gemini image generation is not wired. Paid calls are out of scope for this unit.

use super::{GenerateRequest, ImageProvider, ProviderError};
use crate::raster::RgbaImage;

#[derive(Debug, Default, Clone, Copy)]
pub struct Gemini;

impl ImageProvider for Gemini {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn generate(&self, _request: &GenerateRequest) -> Result<Vec<RgbaImage>, ProviderError> {
        Err(ProviderError::NotConfigured {
            provider: self.name(),
        })
    }
}
