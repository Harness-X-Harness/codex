mod artifact;
mod backend;
mod extension;
mod policy;
mod tool;

#[cfg(test)]
#[path = "grok_composition_tests.rs"]
mod grok_composition_tests;

pub use extension::install;

pub(crate) const IMAGE_GEN_NAMESPACE: &str = "image_gen";
pub(crate) const IMAGEGEN_TOOL_NAME: &str = "imagegen";
