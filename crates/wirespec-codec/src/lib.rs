// wirespec-codec: Backend-neutral Codec IR
pub mod checksum;
pub mod ir;
pub mod lower;
pub mod strategy;

pub use ir::CodecModule;
pub use lower::lower;
