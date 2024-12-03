pub mod version;
pub mod update;

#[cfg(test)]
mod tests;

pub use update::OtaUpdater;
pub use version::VersionManager;


