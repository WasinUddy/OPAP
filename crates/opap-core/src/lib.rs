//! Headless CPAP import and analysis primitives for OPAP.
//!
//! The crate exposes a serializable domain contract and a storage-independent
//! importer boundary. Native applications can use
//! [`importer::DirectorySource`]; future WebAssembly hosts can implement
//! [`importer::ImportSource`] over browser-selected files without changing
//! device parsing logic.

pub mod domain;
pub mod importer;
pub mod resmed;

pub use domain::{
    ChannelKind, ChannelMetadata, DeviceInfo, Event, EventSeries, IMPORT_SCHEMA_VERSION,
    ImportReport, ImportStatistics, ImportWarning, MachineInfo, Session, SessionSummary,
    SessionTimestamp, Setting, SettingValue, SummaryMetric, UnixMillis, WarningSeverity,
    WaveformSeries,
};
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub use importer::DirectorySource;
pub use importer::{
    DeviceDiscovery, ImportError, ImportErrorKind, ImportOptions, ImportSource, Importer,
    SourceEntry, SourceEntryKind, SourceInventory,
};
