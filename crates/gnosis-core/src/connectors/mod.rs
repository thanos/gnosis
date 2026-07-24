pub mod filesystem;
pub mod git;
pub mod types;

pub use filesystem::FilesystemConnector;
pub use git::GitContext;
pub use types::{GitProtoData, ObjectDescriptor, ProtoData};
