//! Image providers. OpenAI and Gemini are stubs: this unit does not call a paid API.
//! Agent mode writes `jobs.json` and waits for image files. It does not open a socket.

mod agent_mode;
mod gemini;
mod openai;

pub use agent_mode::AgentMode;
pub use gemini::Gemini;
pub use openai::OpenAi;

use crate::raster::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Alpha,
    Key([u8; 3]),
}

#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub background: Background,
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    NotConfigured { provider: &'static str },
    Message(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured { provider } => write!(f, "{provider} is not configured"),
            Self::Message(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for ProviderError {}

pub trait ImageProvider {
    fn name(&self) -> &'static str;
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<RgbaImage>, ProviderError>;
}
