//! Metrics for the audit connector.
//!
//! Single endpoint (one audit-archiver per ingress process), so no labels.

base_metrics::define_metrics! {
    audit.connector,
    struct = AuditConnectorMetrics,
    #[describe("Total RPC batches sent successfully to the audit-archiver")]
    batches_sent: counter,
    #[describe("Total bundle events forwarded successfully (server-acked)")]
    events_forwarded: counter,
    #[describe("Total bundle events dropped due to partial server failure (server-acked count < batch size)")]
    events_dropped: counter,
    #[describe("Total RPC send errors after all retries exhausted")]
    rpc_errors: counter,
    #[describe("RPC round-trip latency in seconds (including retries)")]
    rpc_latency: histogram,
    #[describe("Current number of bundle events buffered and awaiting send")]
    buffer_size: gauge,
    #[describe("Total times the inbound event channel was observed closed")]
    channel_closed: counter,
}
