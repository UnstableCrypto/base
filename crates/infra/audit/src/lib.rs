#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    html_favicon_url = "https://avatars.githubusercontent.com/u/16627100?s=200&v=4",
    issue_tracker_base_url = "https://github.com/base/base/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

mod archiver;
pub use archiver::KafkaAuditArchiver;

mod connector;
pub use connector::{
    AuditConnector, AuditConnectorConfig, AuditConnectorMetrics, SpawnedAuditConnector,
};

mod kafka_config;
pub use kafka_config::load_kafka_config_from_file;

mod metrics;
pub use metrics::Metrics;

mod publisher;
pub use publisher::{BundleEventPublisher, KafkaBundleEventPublisher, LoggingBundleEventPublisher};

mod reader;
pub use reader::{
    Event, EventReader, KafkaAuditLogReader, assign_topic_partition, create_kafka_consumer,
};

mod rpc;
pub use rpc::{AuditArchiverApiServer, AuditArchiverRpc};

mod storage;
pub use storage::{
    BundleEventS3Reader, BundleHistory, BundleHistoryEvent, EventWriter, S3EventReaderWriter,
    S3Key, TransactionMetadata,
};

mod types;
pub use types::{BundleEvent, BundleId, DropReason, Transaction, TransactionId};
