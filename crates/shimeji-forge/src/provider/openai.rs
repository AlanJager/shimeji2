//! OpenAI image generation is not wired. Paid calls are out of scope for this unit.

use super::{GenerateRequest, ImageProvider, ProviderError};
use crate::raster::RgbaImage;

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAi;

impl ImageProvider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn generate(&self, _request: &GenerateRequest) -> Result<Vec<RgbaImage>, ProviderError> {
        Err(ProviderError::NotConfigured {
            provider: self.name(),
        })
    }
}
