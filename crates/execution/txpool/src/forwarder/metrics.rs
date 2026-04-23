//! Metrics for the transaction forwarder.

base_metrics::define_metrics! {
    txpool.forwarder,
    struct = ForwarderMetrics,
    #[describe("Total RPC batches sent successfully")]
    #[label(builder_url)]
    batches_sent: counter,
    #[describe("Total individual transactions forwarded")]
    #[label(builder_url)]
    txs_forwarded: counter,
    #[describe("Total RPC send errors (after all retries exhausted)")]
    #[label(builder_url)]
    rpc_errors: counter,
    #[describe("Total number of transactions rejected by the builder's pool within successful batch calls")]
    #[label(builder_url)]
    num_tx_rejected_in_batch: counter,
    #[describe("Total lag events from the broadcast receiver")]
    #[label(builder_url)]
    batches_lagged: counter,
    #[describe("Total individual transactions skipped due to lag")]
    #[label(builder_url)]
    txs_lagged: counter,
    #[describe("RPC round-trip latency in seconds (including retries)")]
    #[label(builder_url)]
    rpc_latency: histogram,
    #[describe("Current number of transactions buffered and awaiting send")]
    #[label(builder_url)]
    buffer_size: gauge,
    #[describe("Total audit batches sent successfully (base_persistEventBatch)")]
    #[label(builder_url)]
    audit_batches_sent: counter,
    #[describe("Total audit RPC errors (base_persistEventBatch)")]
    #[label(builder_url)]
    audit_errors: counter,
    #[describe("Total individual MempoolForwarded events successfully persisted by audit")]
    #[label(builder_url)]
    audit_txs_persisted: counter,
    #[describe("Audit RPC round-trip latency in seconds (base_persistEventBatch)")]
    #[label(builder_url)]
    audit_rpc_latency: histogram,
    #[describe("Total individual MempoolForwarded events dropped due to audit RPC failures")]
    #[label(builder_url)]
    audit_events_dropped: counter,
}
