//! FMUCD CSV ingestion and mapping to readiness events.

pub mod aggregator;
pub mod config;
pub mod loader;
pub mod mapper;
pub mod schema;

pub use aggregator::{
    aggregate_by_asset, dataset_summary, evaluate_dataset_documentary, evaluate_dataset_dual,
    evaluate_dataset_strict, summarize_by_university, AggregatedDataset,
};
pub use config::FmucdMappingConfig;
pub use loader::{load_fmucd_csv, LoadProgress};
pub use mapper::map_row_to_event;
pub use schema::FmucdRow;
